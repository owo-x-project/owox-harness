//! スキル正本層。`.owox/skills/<id>/` を読み、テスト合格ゲートで登録集合を出す。
//!
//! スキルは横断標準 (SKILL.md) + owox 固有設定 (owox.toml) で構成する
//! (`docs/decisions/20260613-Phase5-スキルと入口.md`)。owox の上乗せは
//! テスト合格ゲート (Codex に無い) と昇格の人間ゲート。
//!
//! - SKILL.md: 横断標準。frontmatter は name / description、本文は手順。生成物へ verbatim で出す
//! - owox.toml: owox 固有 (implicit / human_gate / promoted)。SKILL.md を標準のまま保つため分離
//! - tests/: 実行可能なテスト。全て終了コード 0 で合格 (verify の run_check 再利用)
//! - scripts/: 任意。生成物へ同梱する
//! - memory.md: 任意。経験メモリ。生成物へは出さない (正本のみ)
//!
//! 自動起動 (implicit) は「意図」。実際の発火は implicit かつ promoted (昇格) の両方で解禁する。
//! テスト実行 (副作用) は読込と分け、登録集合の算出時に走らせる。

use std::path::{Path, PathBuf};

use regex::Regex;
use serde_json::json;

use crate::envelope::Envelope;
use crate::markdown::split_pair;
use crate::record::{DecisionLinks, DecisionStatus, RecordInput, record_decision};
use crate::target::{find, write_all};
use crate::verify::{CheckResult, run_checks};

/// スキル 1 件。`.owox/skills/<id>/` の型付き表現。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skill {
    /// ディレクトリ名 = 識別子。
    pub id: String,
    /// frontmatter の name。
    pub name: String,
    /// frontmatter の description (実行条件・範囲)。
    pub description: String,
    /// SKILL.md 全文。生成物へ verbatim で出す (横断標準)。
    pub skill_md: String,
    /// 自動起動してよいという意図 (owox.toml。既定 true)。
    pub implicit: bool,
    /// 昇格済みか (owox.toml。既定 false。skill.promote が人間承認後に立てる)。
    pub promoted: bool,
    /// 不可逆操作を含む等で人間確認が要るか (owox.toml。既定 false)。
    pub human_gate: bool,
    /// tests/ 配下のテストファイル (絶対パス。実行可能)。
    pub tests: Vec<PathBuf>,
    /// scripts/ 配下の同梱物。
    pub scripts: Vec<ScriptFile>,
}

/// 同梱スクリプト 1 件。生成物へそのまま書く。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptFile {
    /// skill dir からの相対パス (例 "scripts/foo.sh")。
    pub rel: String,
    /// 内容。
    pub contents: String,
    /// 実行ビットを持つか。
    pub executable: bool,
}

impl Skill {
    /// 実際に自動起動を解禁してよいか。意図 (implicit) かつ昇格 (promoted) の両方。
    /// 人間が信頼 (昇格) するまで勝手に発火させない。
    pub fn effective_implicit(&self) -> bool {
        self.implicit && self.promoted
    }

    /// 登録に必要な妥当性 = 契約 lint を満たすか。満たさない理由があれば Err
    /// (`docs/decisions/20260616-Phase8-スキルテスト是正.md`)。
    ///
    /// owox が自動で走らせる決定的検査。人間が書かない最低ゲート。
    /// - name / description は必須 (横断標準)
    /// - implicit=true は tests/ 必須 (自動起動する技は機械検証で守る)
    /// - SKILL.md 本文が参照する `scripts/<name>` が実在する (パス形を抽出し ScriptFile と照合)
    /// - tests/ の各ファイルが実行ビットを持つ (sh 起動が誤らない)
    fn registrable_shape(&self) -> Result<(), String> {
        if self.name.trim().is_empty() {
            return Err("SKILL.md の frontmatter に name が無い".to_string());
        }
        if self.description.trim().is_empty() {
            return Err("SKILL.md の frontmatter に description が無い".to_string());
        }
        if self.implicit && self.tests.is_empty() {
            return Err("implicit=true のスキルは tests/ が必須".to_string());
        }
        // SKILL.md が参照する scripts/<name> は同梱されていなければならない。
        for referenced in script_refs(&self.skill_md) {
            if !self.scripts.iter().any(|s| s.rel == referenced) {
                return Err(format!(
                    "SKILL.md が参照する {referenced} が scripts/ に無い (同梱するか参照を直す)"
                ));
            }
        }
        // tests/ の各ファイルは実行可能でなければ起動が誤る。
        for test in &self.tests {
            if !is_executable(test) {
                let name = test.file_name().and_then(|s| s.to_str()).unwrap_or("test");
                return Err(format!("tests/{name} に実行ビットが無い (chmod +x が要る)"));
            }
        }
        Ok(())
    }

    /// 登録は可だが確認を促す助言 (warning) を集める。
    ///
    /// 3 条件:
    /// 1. script があるのに tests が無い (implicit=false の明示 skill は登録可だが warning)
    /// 2. script があるのに SKILL.md で script 名への言及が無い (draft 扱いにする)
    /// 3. tests が script を呼んでいない (tests ファイル本文に script 名が現れない。簡易 grep)
    ///
    /// 条件2は stage を draft に下げるため registrable_shape の判定より後に評価する。
    /// この関数は warning だけを返す (stage 降格は skill_status が担う)。
    pub(crate) fn registration_warnings(&self) -> Vec<String> {
        let mut warnings = Vec::new();

        if self.scripts.is_empty() {
            // script が無ければ 3 条件すべて対象外。
            return warnings;
        }

        // 条件1: script があるのに tests が無い (明示 skill = implicit=false のみ対象。
        //        implicit=true は registrable_shape で既に弾く)。
        if self.tests.is_empty() && !self.implicit {
            warnings.push(
                "scripts/ があるのに tests/ が無い。script の動作を最低限テストで保証することを推奨"
                    .to_string(),
            );
        }

        // 条件2: script があるのに SKILL.md で script 名への言及が無い。
        // いずれの script 名も SKILL.md 本文に現れなければ draft 候補 (stage 降格は skill_status)。
        let any_script_mentioned = self.scripts.iter().any(|s| {
            let file_name = s.rel.split('/').next_back().unwrap_or(&s.rel);
            self.skill_md.contains(file_name)
        });
        if !any_script_mentioned {
            warnings.push(
                "scripts/ があるのに SKILL.md に script 名の言及が無い。使い方を SKILL.md へ追記することを推奨 (draft 扱い)"
                    .to_string(),
            );
        }

        // 条件3: tests が script を呼んでいない (tests ファイル本文に script 名が現れない)。
        if !self.tests.is_empty() {
            for script in &self.scripts {
                let file_name = script.rel.split('/').next_back().unwrap_or(&script.rel);
                let test_calls_script = self.tests.iter().any(|test_path| {
                    std::fs::read_to_string(test_path)
                        .map(|content| content.contains(file_name))
                        .unwrap_or(false)
                });
                if !test_calls_script {
                    warnings.push(format!(
                        "tests/ が {file_name} を呼んでいない可能性がある。テストで script を実行することを推奨"
                    ));
                }
            }
        }

        warnings
    }
}

/// スキル 1 件の状態。skill.list の可視化に使う 2 軸。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillStatus {
    pub id: String,
    pub name: String,
    /// 昇格軸: draft / registered / promoted。
    pub stage: Stage,
    /// テスト軸: passing / failing / none。
    pub tests: TestState,
    /// registrable_shape が返した不適格の理由 (あれば)。
    pub problem: Option<String>,
    /// 登録ゲート追加助言 (warning)。登録は可だが人間が確認すべき点。
    pub warnings: Vec<String>,
}

/// 昇格軸の状態。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    /// 未登録 (テスト未合格・不適格)。Codex から見えない。
    Draft,
    /// テスト合格・適格。生成され明示起動で使える。実験段階。
    Registered,
    /// 人間が昇格させた。自動起動 (implicit 意図があれば) 解禁。
    Promoted,
}

impl Stage {
    pub fn as_str(self) -> &'static str {
        match self {
            Stage::Draft => "draft",
            Stage::Registered => "registered",
            Stage::Promoted => "promoted",
        }
    }
}

/// テスト軸の状態。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TestState {
    Passing,
    Failing,
    None,
}

impl TestState {
    pub fn as_str(self) -> &'static str {
        match self {
            TestState::Passing => "passing",
            TestState::Failing => "failing",
            TestState::None => "none",
        }
    }
}

/// `.owox/skills/`。
fn skills_dir(owox_dir: &Path) -> PathBuf {
    owox_dir.join("skills")
}

/// 全スキルを読む。`.owox/skills/<id>/`。ディレクトリが無ければ空。
///
/// テストは走らせない (副作用を読込から分ける)。frontmatter 検証は status / 登録時に行う。
pub fn load_skills(owox_dir: &Path) -> Result<Vec<Skill>, String> {
    let dir = skills_dir(owox_dir);
    let read = match std::fs::read_dir(&dir) {
        Ok(read) => read,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(format!("{} を読めない: {err}", dir.display())),
    };

    let mut skills = Vec::new();
    for entry in read {
        let path = entry.map_err(|e| e.to_string())?.path();
        if !path.is_dir() {
            continue;
        }
        let id = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or_default()
            .to_string();
        skills.push(load_one(&path, &id)?);
    }
    // id で安定に並べる (生成・一覧の決定論)。
    skills.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(skills)
}

/// 1 スキルを読む。SKILL.md は必須。
fn load_one(dir: &Path, id: &str) -> Result<Skill, String> {
    let skill_md_path = dir.join("SKILL.md");
    let skill_md = std::fs::read_to_string(&skill_md_path)
        .map_err(|e| format!("{} を読めない: {e}", skill_md_path.display()))?;
    let (name, description) = parse_frontmatter(&skill_md);

    let owox_toml_path = dir.join("owox.toml");
    let (implicit, promoted, human_gate) = match std::fs::read_to_string(&owox_toml_path) {
        Ok(text) => parse_owox_toml(&text)
            .map_err(|e| format!("{} を解釈できない: {e}", owox_toml_path.display()))?,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => owox_defaults(),
        Err(err) => return Err(format!("{} を読めない: {err}", owox_toml_path.display())),
    };

    let tests = read_tests(&dir.join("tests"))?;
    let scripts = read_scripts(&dir.join("scripts"))?;

    Ok(Skill {
        id: id.to_string(),
        name,
        description,
        skill_md,
        implicit,
        promoted,
        human_gate,
        tests,
        scripts,
    })
}

/// owox.toml の既定。技系 (`.owox/skills/`) は自動起動の意図を既定 true、昇格は false。
fn owox_defaults() -> (bool, bool, bool) {
    (true, false, false)
}

/// owox.toml を読む。implicit / promoted / human_gate (全て任意・bool)。未知キーは弾く。
fn parse_owox_toml(text: &str) -> Result<(bool, bool, bool), String> {
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Raw {
        implicit: Option<bool>,
        promoted: Option<bool>,
        human_gate: Option<bool>,
    }
    let raw: Raw = toml::from_str(text).map_err(|e| e.to_string())?;
    let (di, dp, dh) = owox_defaults();
    Ok((
        raw.implicit.unwrap_or(di),
        raw.promoted.unwrap_or(dp),
        raw.human_gate.unwrap_or(dh),
    ))
}

/// SKILL.md の frontmatter から name と description を取り出す。
///
/// frontmatter は先頭の `---` 行から次の `---` 行まで。中は `key: value` の平坦形
/// (横断標準の必須は name / description のみ。serde_yaml 依存を避け自作で読む)。
/// frontmatter が無ければ両方空 (登録時に弾く)。
fn parse_frontmatter(text: &str) -> (String, String) {
    let mut lines = text.lines();
    // 先頭の空行を飛ばし、最初の意味行が `---` でなければ frontmatter 無し。
    let mut started = false;
    let mut name = String::new();
    let mut description = String::new();
    for line in lines.by_ref() {
        let trimmed = line.trim();
        if !started {
            if trimmed.is_empty() {
                continue;
            }
            if trimmed == "---" {
                started = true;
                continue;
            }
            // frontmatter が始まらない。
            return (String::new(), String::new());
        }
        if trimmed == "---" {
            break;
        }
        let (key, value) = split_pair(trimmed);
        match key.as_str() {
            "name" => name = value,
            "description" => description = value,
            _ => {} // 他キーは無視 (横断標準は name/description のみ必須)。
        }
    }
    (name, description)
}

/// tests/ 配下の実行ファイルを集める (絶対パス・id 順)。ディレクトリが無ければ空。
fn read_tests(dir: &Path) -> Result<Vec<PathBuf>, String> {
    let read = match std::fs::read_dir(dir) {
        Ok(read) => read,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(format!("{} を読めない: {err}", dir.display())),
    };
    let mut tests = Vec::new();
    for entry in read {
        let path = entry.map_err(|e| e.to_string())?.path();
        if path.is_file() {
            tests.push(path);
        }
    }
    tests.sort();
    Ok(tests)
}

/// scripts/ 配下のファイルを集める (rel パス・内容・実行ビット)。ディレクトリが無ければ空。
fn read_scripts(dir: &Path) -> Result<Vec<ScriptFile>, String> {
    let read = match std::fs::read_dir(dir) {
        Ok(read) => read,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(format!("{} を読めない: {err}", dir.display())),
    };
    let mut scripts = Vec::new();
    for entry in read {
        let path = entry.map_err(|e| e.to_string())?.path();
        if !path.is_file() {
            continue;
        }
        let file_name = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or_default()
            .to_string();
        let contents = std::fs::read_to_string(&path)
            .map_err(|e| format!("{} を読めない: {e}", path.display()))?;
        scripts.push(ScriptFile {
            rel: format!("scripts/{file_name}"),
            contents,
            executable: is_executable(&path),
        });
    }
    scripts.sort_by(|a, b| a.rel.cmp(&b.rel));
    Ok(scripts)
}

/// SKILL.md 本文から `scripts/<name>` のパス形参照を機械抽出する (契約 lint 用)。
///
/// 保守的にパス形 (`scripts/` 前置 + ファイル名文字) だけ拾う。散文中の tool 名や `scripts/` 単独
/// (末尾にファイル名が無い) は対象にしない (過検出を避ける)。末尾の句読点は落とす。
fn script_refs(skill_md: &str) -> Vec<String> {
    // scripts/ の直後にファイル名文字 (英数・`.` `_` `-` `/`) が 1 文字以上続く形だけ拾う。
    let re = Regex::new(r"scripts/[A-Za-z0-9._/-]+").expect("script ref pattern is valid");
    let mut refs: Vec<String> = re
        .find_iter(skill_md)
        .map(|m| {
            m.as_str()
                .trim_end_matches(['.', ',', ';', ':', ')'])
                .to_string()
        })
        // 末尾を落とした結果 `scripts/` だけになった (パス形でない) ものは捨てる。
        .filter(|r| r.len() > "scripts/".len())
        .collect();
    refs.sort();
    refs.dedup();
    refs
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .map(|m| m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable(_path: &Path) -> bool {
    false
}

/// スキルのテストを実行する。tests/ の各ファイルを 1 検査として repo ルートで走らせる。
///
/// `repo_root` は検査を実行する場所 (target repo ルート = `.owox` の親)。verify と同じ。
pub fn run_skill_tests(skill: &Skill, repo_root: &Path) -> Vec<CheckResult> {
    let checks: Vec<crate::model::VerifyCheck> = skill
        .tests
        .iter()
        .map(|p| crate::model::VerifyCheck {
            name: p
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("test")
                .to_string(),
            command: p.display().to_string(),
            evidence_paths: Vec::new(),
        })
        .collect();
    run_checks(repo_root, &checks)
}

/// スキルの状態 (2 軸) を算出する。テストを実行する (副作用)。
pub fn skill_status(skill: &Skill, repo_root: &Path) -> SkillStatus {
    let problem = skill.registrable_shape().err();

    let tests = if skill.tests.is_empty() {
        TestState::None
    } else if run_skill_tests(skill, repo_root).iter().all(|r| r.passed) {
        TestState::Passing
    } else {
        TestState::Failing
    };

    let warnings = skill.registration_warnings();

    // SKILL.md に script 名の言及が無い場合は draft 扱い (warning の中から判断)。
    let missing_skill_md_mention = warnings
        .iter()
        .any(|w| w.contains("SKILL.md に script 名の言及が無い"));

    // 登録可: 不適格でなく、テストが失敗していない (none か passing) かつ draft 降格条件なし。
    let registrable = problem.is_none() && tests != TestState::Failing && !missing_skill_md_mention;
    let stage = if registrable && skill.promoted {
        Stage::Promoted
    } else if registrable {
        Stage::Registered
    } else {
        Stage::Draft
    };

    SkillStatus {
        id: skill.id.clone(),
        name: skill.name.clone(),
        stage,
        tests,
        problem,
        warnings,
    }
}

/// 登録対象 (生成してよい) スキルを返す。テストを実行する (副作用)。
///
/// 登録 = 妥当 (name/description・implicit→tests 必須) かつ テスト非失敗。
/// setup と skill.register が共用する。
pub fn registered_skills(owox_dir: &Path, repo_root: &Path) -> Result<Vec<Skill>, String> {
    let skills = load_skills(owox_dir)?;
    Ok(skills
        .into_iter()
        .filter(|s| {
            let st = skill_status(s, repo_root);
            st.stage != Stage::Draft
        })
        .collect())
}

/// skill.list。全スキルの 2 軸状態を封筒で返す。テストを実行する (副作用)。
pub fn list_skills_envelope(owox_dir: &Path, repo_root: &Path) -> Envelope {
    let skills = match load_skills(owox_dir) {
        Ok(s) => s,
        Err(err) => return Envelope::failed(err),
    };
    let items: Vec<_> = skills
        .iter()
        .map(|s| {
            let st = skill_status(s, repo_root);
            json!({
                "id": st.id,
                "name": st.name,
                "stage": st.stage.as_str(),
                "tests": st.tests.as_str(),
                "problem": st.problem,
                "warnings": st.warnings,
            })
        })
        .collect();
    Envelope::ok(
        format!("{} skill(s).", items.len()),
        json!({ "skills": items }),
    )
}

/// skill.register。指定スキルのテストを実行し、合格・適格なら生成 (登録)、不適格なら失敗を返す。
pub fn register_skill(owox_dir: &Path, repo_root: &Path, id: &str) -> Envelope {
    let skill = match find_skill(owox_dir, id) {
        Ok(Some(s)) => s,
        Ok(None) => return Envelope::failed(format!("skill が無い: {id}")),
        Err(err) => return Envelope::failed(err),
    };

    let status = skill_status(&skill, repo_root);
    if status.stage == Stage::Draft {
        let detail = status
            .problem
            .clone()
            .unwrap_or_else(|| "tests failed".to_string());
        return Envelope::failed(format!("skill {id} を登録できない: {detail}"))
            .with_data(
                json!({ "id": id, "tests": status.tests.as_str(), "problem": status.problem }),
            )
            .with_next_actions(vec![
                "Fix the skill (tests or frontmatter), then register again.".to_string(),
            ]);
    }

    match write_skills_to_targets(owox_dir, repo_root, &[skill]) {
        Ok(written) => {
            let mut env = Envelope::ok(
                format!("Registered skill {id} ({}).", status.stage.as_str()),
                json!({ "id": id, "stage": status.stage.as_str(), "written": written, "warnings": status.warnings }),
            );
            if !status.warnings.is_empty() {
                env = env.with_next_actions(
                    status
                        .warnings
                        .iter()
                        .map(|w| format!("Warning: {w}"))
                        .collect(),
                );
            }
            env
        }
        Err(err) => Envelope::failed(err),
    }
}

/// skill.promote。人間承認後に使う昇格ゲート (gate.approve と同じ約束)。
///
/// promoted を立て、来歴へ記録し、生成し直す (openai.yaml が implicit を解禁する値へ更新)。
/// 未登録 (draft) は昇格できない (先に登録する)。
pub fn promote_skill(owox_dir: &Path, repo_root: &Path, today: &str, id: &str) -> Envelope {
    let mut skill = match find_skill(owox_dir, id) {
        Ok(Some(s)) => s,
        Ok(None) => return Envelope::failed(format!("skill が無い: {id}")),
        Err(err) => return Envelope::failed(err),
    };

    if skill_status(&skill, repo_root).stage == Stage::Draft {
        return Envelope::failed(format!(
            "skill {id} は未登録 (draft) のため昇格できない。先に skill.register で登録する"
        ));
    }
    if skill.promoted {
        return Envelope::ok(
            format!("skill {id} は既に昇格済み"),
            json!({ "id": id, "stage": "promoted" }),
        );
    }

    // promoted を立てて owox.toml へ永続化する。
    skill.promoted = true;
    if let Err(err) = write_owox_toml(owox_dir, &skill) {
        return Envelope::failed(err);
    }

    // 来歴へ記録する (昇格 = 正本への人間ゲート)。
    let rec = record_decision(
        owox_dir,
        today,
        RecordInput {
            title: format!("Promote skill {id}"),
            status: DecisionStatus::Adopted,
            rationale: "Human-approved promotion. Auto-invocation is enabled if the skill opts into implicit.".to_string(),
            links: DecisionLinks::default(),
            supersedes: Vec::new(),
        },
    );
    let decision_ids = rec.decision_ids.clone();

    // 生成し直して openai.yaml を更新する (昇格を反映)。
    match write_skills_to_targets(owox_dir, repo_root, &[skill]) {
        Ok(written) => Envelope::ok(
            format!("Promoted skill {id}. Auto-invocation now follows its implicit setting."),
            json!({ "id": id, "stage": "promoted", "written": written }),
        )
        .with_decision_ids(decision_ids),
        Err(err) => Envelope::failed(err),
    }
}

/// skill.remember。経験メモリ (memory.md) へ追記する。task.note の skill 版。
pub fn remember(owox_dir: &Path, today: &str, id: &str, text: &str) -> Envelope {
    if text.trim().is_empty() {
        return Envelope::failed("memory text が空");
    }
    let dir = skills_dir(owox_dir).join(id);
    if !dir.is_dir() {
        return Envelope::failed(format!("skill が無い: {id}"));
    }
    let path = dir.join("memory.md");
    let mut body = std::fs::read_to_string(&path).unwrap_or_default();
    if body.is_empty() {
        body.push_str("# Experience memory\n\n");
    }
    body.push_str(&format!("- {today}: {}\n", text.trim()));
    if let Err(err) = std::fs::write(&path, body) {
        return Envelope::failed(format!("{} へ書けない: {err}", path.display()));
    }
    Envelope::ok(
        format!("Recorded experience for skill {id}."),
        json!({ "id": id }),
    )
}

/// id のスキルを 1 件読む。無ければ None。
fn find_skill(owox_dir: &Path, id: &str) -> Result<Option<Skill>, String> {
    let dir = skills_dir(owox_dir).join(id);
    if !dir.is_dir() {
        return Ok(None);
    }
    load_one(&dir, id).map(Some)
}

/// 登録対象スキルを設定された各 target の配置へ書く。書いたパスを返す。
/// register と promote が共用する (setup は registered_skills + 各 target を直接呼ぶ)。
fn write_skills_to_targets(
    owox_dir: &Path,
    repo_root: &Path,
    skills: &[Skill],
) -> Result<Vec<String>, String> {
    let canon = crate::load::load_canon(owox_dir).map_err(|e| e.to_string())?;
    let targets: Vec<(String, String)> = if canon.targets.entries.is_empty() {
        vec![("codex".to_string(), ".".to_string())]
    } else {
        canon
            .targets
            .entries
            .iter()
            .map(|t| (t.name.clone(), t.out_dir.clone()))
            .collect()
    };

    let mut written = Vec::new();
    for (name, out) in targets {
        let target = find(&name).ok_or_else(|| format!("未知の対象 CLI: {name}"))?;
        let files = target.generate_skills(skills);
        let out_root = repo_root.join(&out);
        write_all(&out_root, &files).map_err(|e| e.to_string())?;
        for f in &files {
            written.push(out_root.join(&f.path).display().to_string());
        }
    }
    Ok(written)
}

/// skill の owox.toml を現在の値で書き直す (promote の永続化)。
fn write_owox_toml(owox_dir: &Path, skill: &Skill) -> Result<(), String> {
    let path = skills_dir(owox_dir).join(&skill.id).join("owox.toml");
    let body = format!(
        "implicit = {}\npromoted = {}\nhuman_gate = {}\n",
        skill.implicit, skill.promoted, skill.human_gate
    );
    std::fs::write(&path, body).map_err(|e| format!("{} へ書けない: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tempdir() -> PathBuf {
        use std::sync::atomic::{AtomicU32, Ordering};
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let pid = std::process::id();
        let dir = std::env::temp_dir().join(format!("owox-skill-test-{pid}-{n}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// skill dir を作る。SKILL.md・任意の owox.toml・任意のテスト (合否指定)。
    fn make_skill(
        owox: &Path,
        id: &str,
        frontmatter: &str,
        owox_toml: Option<&str>,
        test: Option<&str>,
    ) {
        let dir = owox.join("skills").join(id);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("SKILL.md"), frontmatter).unwrap();
        if let Some(t) = owox_toml {
            std::fs::write(dir.join("owox.toml"), t).unwrap();
        }
        if let Some(cmd) = test {
            let tdir = dir.join("tests");
            std::fs::create_dir_all(&tdir).unwrap();
            let tpath = tdir.join("t.sh");
            std::fs::write(&tpath, format!("#!/bin/sh\n{cmd}\n")).unwrap();
            // tests/ は実行可能が前提 (絶対パスを sh -c で起動する)。
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&tpath, std::fs::Permissions::from_mode(0o755)).unwrap();
            }
        }
    }

    const FM_OK: &str = "---\nname: tidy\ndescription: tidy up code\n---\n\nDo the thing.\n";

    #[test]
    fn parse_frontmatter_reads_name_and_description() {
        let (n, d) = parse_frontmatter(FM_OK);
        assert_eq!(n, "tidy");
        assert_eq!(d, "tidy up code");
    }

    #[test]
    fn parse_frontmatter_missing_is_empty() {
        let (n, d) = parse_frontmatter("# no frontmatter\n\nbody");
        assert!(n.is_empty() && d.is_empty());
    }

    #[test]
    fn missing_dir_is_empty() {
        let owox = tempdir();
        assert!(load_skills(&owox).unwrap().is_empty());
    }

    #[test]
    fn loads_defaults_without_owox_toml() {
        let owox = tempdir();
        make_skill(&owox, "tidy", FM_OK, None, Some("exit 0"));
        let skills = load_skills(&owox).unwrap();
        assert_eq!(skills.len(), 1);
        let s = &skills[0];
        assert_eq!(s.id, "tidy");
        assert_eq!(s.name, "tidy");
        assert!(s.implicit); // 既定 true
        assert!(!s.promoted); // 既定 false
        assert!(!s.effective_implicit()); // 昇格前は発火しない
    }

    #[test]
    fn passing_tests_register_failing_stay_draft() {
        let owox = tempdir();
        let repo = owox.parent().unwrap();
        make_skill(&owox, "good", FM_OK, None, Some("exit 0"));
        make_skill(&owox, "bad", FM_OK, None, Some("exit 1"));
        let skills = load_skills(&owox).unwrap();
        let good = skills.iter().find(|s| s.id == "good").unwrap();
        let bad = skills.iter().find(|s| s.id == "bad").unwrap();
        assert_eq!(skill_status(good, repo).stage, Stage::Registered);
        assert_eq!(skill_status(good, repo).tests, TestState::Passing);
        assert_eq!(skill_status(bad, repo).stage, Stage::Draft);
        assert_eq!(skill_status(bad, repo).tests, TestState::Failing);
    }

    #[test]
    fn implicit_without_tests_is_draft() {
        let owox = tempdir();
        let repo = owox.parent().unwrap();
        // implicit=true (既定) かつ tests 無し → 不適格で draft。
        make_skill(&owox, "noimpl", FM_OK, None, None);
        let s = &load_skills(&owox).unwrap()[0];
        let st = skill_status(s, repo);
        assert_eq!(st.stage, Stage::Draft);
        assert!(st.problem.as_deref().unwrap().contains("tests/"));
    }

    #[test]
    fn explicit_without_tests_registers() {
        let owox = tempdir();
        let repo = owox.parent().unwrap();
        // implicit=false なら tests 免除。tests 無しでも登録可。
        make_skill(&owox, "cmdlike", FM_OK, Some("implicit = false\n"), None);
        let s = &load_skills(&owox).unwrap()[0];
        assert_eq!(skill_status(s, repo).stage, Stage::Registered);
    }

    #[test]
    fn promoted_skill_is_promoted_and_fires() {
        let owox = tempdir();
        let repo = owox.parent().unwrap();
        make_skill(
            &owox,
            "trusted",
            FM_OK,
            Some("implicit = true\npromoted = true\n"),
            Some("exit 0"),
        );
        let s = &load_skills(&owox).unwrap()[0];
        assert_eq!(skill_status(s, repo).stage, Stage::Promoted);
        assert!(s.effective_implicit()); // implicit かつ promoted で発火解禁
    }

    #[test]
    fn missing_frontmatter_is_draft() {
        let owox = tempdir();
        let repo = owox.parent().unwrap();
        make_skill(&owox, "bare", "no frontmatter here", None, Some("exit 0"));
        let s = &load_skills(&owox).unwrap()[0];
        let st = skill_status(s, repo);
        assert_eq!(st.stage, Stage::Draft);
        assert!(st.problem.is_some());
    }

    #[test]
    fn script_refs_extracts_path_forms_only() {
        // パス形は拾う。
        let r = script_refs("Run `scripts/build.sh` then scripts/lint.py --fix.");
        assert_eq!(r, vec!["scripts/build.sh", "scripts/lint.py"]);
        // 散文中の `scripts/` 単独や tool 名は拾わない。
        assert!(script_refs("Put helpers in the scripts/ directory.").is_empty());
        assert!(script_refs("Call the bash tool to run things.").is_empty());
    }

    /// skill dir へ scripts/<name> を書く (実行ビットつき)。
    fn write_script(owox: &Path, id: &str, name: &str) {
        let dir = owox.join("skills").join(id).join("scripts");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        std::fs::write(&path, "#!/bin/sh\nexit 0\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
    }

    #[test]
    fn missing_referenced_script_is_draft() {
        let owox = tempdir();
        let repo = owox.parent().unwrap();
        // scripts/run.sh を参照するが同梱しない。implicit=false で tests 要件は外す。
        let fm = "---\nname: t\ndescription: d\n---\n\nRun `scripts/run.sh`.\n";
        make_skill(&owox, "ref", fm, Some("implicit = false\n"), None);
        let s = &load_skills(&owox).unwrap()[0];
        let st = skill_status(s, repo);
        assert_eq!(st.stage, Stage::Draft);
        assert!(st.problem.as_deref().unwrap().contains("scripts/run.sh"));
    }

    #[test]
    fn present_referenced_script_registers() {
        let owox = tempdir();
        let repo = owox.parent().unwrap();
        let fm = "---\nname: t\ndescription: d\n---\n\nRun `scripts/run.sh`.\n";
        make_skill(&owox, "ref", fm, Some("implicit = false\n"), None);
        write_script(&owox, "ref", "run.sh");
        let s = &load_skills(&owox).unwrap()[0];
        assert_eq!(skill_status(s, repo).stage, Stage::Registered);
    }

    #[test]
    fn non_executable_test_is_draft() {
        let owox = tempdir();
        let repo = owox.parent().unwrap();
        // tests/t.sh を実行ビット無しで置く。
        let dir = owox.join("skills").join("noexec");
        std::fs::create_dir_all(dir.join("tests")).unwrap();
        std::fs::write(dir.join("SKILL.md"), FM_OK).unwrap();
        let tpath = dir.join("tests").join("t.sh");
        std::fs::write(&tpath, "#!/bin/sh\nexit 0\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&tpath, std::fs::Permissions::from_mode(0o644)).unwrap();
        }
        let s = &load_skills(&owox).unwrap()[0];
        let st = skill_status(s, repo);
        assert_eq!(st.stage, Stage::Draft);
        assert!(st.problem.as_deref().unwrap().contains("実行ビット"));
    }

    #[test]
    fn registered_skills_filters_drafts() {
        let owox = tempdir();
        let repo = owox.parent().unwrap();
        make_skill(&owox, "good", FM_OK, None, Some("exit 0"));
        make_skill(&owox, "bad", FM_OK, None, Some("exit 1"));
        let reg = registered_skills(&owox, repo).unwrap();
        assert_eq!(reg.len(), 1);
        assert_eq!(reg[0].id, "good");
    }

    /// repo ルート (= tempdir) と その中の `.owox` を用意する。
    /// 生成物 (.agents/skills) が repo 内へ収まるよう、owox の親を repo にする。
    fn repo_and_owox() -> (PathBuf, PathBuf) {
        let repo = tempdir();
        let owox = repo.join(".owox");
        std::fs::create_dir_all(&owox).unwrap();
        std::fs::write(owox.join("brand.md"), "# B\n\n## Vision\n\nv\n").unwrap();
        (repo, owox)
    }

    #[test]
    fn register_generates_passing_skill() {
        let (repo, owox) = repo_and_owox();
        make_skill(&owox, "good", FM_OK, None, Some("exit 0"));
        let env = register_skill(&owox, &repo, "good");
        assert_eq!(env.status, crate::envelope::Status::Ok);
        assert!(repo.join(".agents/skills/good/SKILL.md").is_file());
    }

    #[test]
    fn register_rejects_failing_skill() {
        let (repo, owox) = repo_and_owox();
        make_skill(&owox, "bad", FM_OK, None, Some("exit 1"));
        let env = register_skill(&owox, &repo, "bad");
        assert_eq!(env.status, crate::envelope::Status::Failed);
        // 失敗スキルは生成されない。
        assert!(!repo.join(".agents/skills/bad/SKILL.md").exists());
    }

    #[test]
    fn register_missing_skill_fails() {
        let (repo, owox) = repo_and_owox();
        assert_eq!(
            register_skill(&owox, &repo, "nope").status,
            crate::envelope::Status::Failed
        );
    }

    #[test]
    fn promote_sets_flag_records_and_enables_implicit() {
        let (repo, owox) = repo_and_owox();
        make_skill(&owox, "good", FM_OK, None, Some("exit 0"));
        let env = promote_skill(&owox, &repo, "20260613", "good");
        assert_eq!(env.status, crate::envelope::Status::Ok);
        // owox.toml に promoted=true が永続化される。
        let s = &load_skills(&owox).unwrap()[0];
        assert!(s.promoted);
        assert!(s.effective_implicit());
        // 来歴へ記録される。
        assert_eq!(crate::record::list_decisions(&owox).unwrap().len(), 1);
        // 生成し直され openai.yaml が implicit を解禁する。
        let yaml =
            std::fs::read_to_string(repo.join(".agents/skills/good/agents/openai.yaml")).unwrap();
        assert!(yaml.contains("allow_implicit_invocation: true"));
    }

    #[test]
    fn promote_draft_fails() {
        let (repo, owox) = repo_and_owox();
        make_skill(&owox, "bad", FM_OK, None, Some("exit 1"));
        assert_eq!(
            promote_skill(&owox, &repo, "20260613", "bad").status,
            crate::envelope::Status::Failed
        );
    }

    #[test]
    fn remember_appends_memory() {
        let (_repo, owox) = repo_and_owox();
        make_skill(&owox, "good", FM_OK, None, Some("exit 0"));
        let env = remember(&owox, "20260613", "good", "tests flaked on slow IO");
        assert_eq!(env.status, crate::envelope::Status::Ok);
        let mem = std::fs::read_to_string(owox.join("skills/good/memory.md")).unwrap();
        assert!(mem.contains("tests flaked on slow IO"));
        assert!(mem.contains("20260613"));
    }

    #[test]
    fn remember_missing_skill_fails() {
        let (_repo, owox) = repo_and_owox();
        assert_eq!(
            remember(&owox, "20260613", "nope", "x").status,
            crate::envelope::Status::Failed
        );
    }

    #[test]
    fn list_envelope_reports_all_skills() {
        let (repo, owox) = repo_and_owox();
        make_skill(&owox, "good", FM_OK, None, Some("exit 0"));
        make_skill(&owox, "bad", FM_OK, None, Some("exit 1"));
        let env = list_skills_envelope(&owox, &repo);
        assert_eq!(env.status, crate::envelope::Status::Ok);
        let skills = env.data.unwrap()["skills"].as_array().unwrap().len();
        assert_eq!(skills, 2);
    }

    // --- 追加テスト: 登録ゲート3助言 ---

    /// script があるのに tests が無い明示 skill は warning 付きで登録可。
    #[test]
    fn explicit_skill_with_script_no_tests_registers_with_warning() {
        let owox = tempdir();
        let repo = owox.parent().unwrap();
        // implicit=false (明示 skill)、script あり、tests なし。
        let fm = "---\nname: t\ndescription: d\n---\n\nRun `scripts/run.sh`.\n";
        make_skill(&owox, "explicit", fm, Some("implicit = false\n"), None);
        write_script(&owox, "explicit", "run.sh");
        let s = &load_skills(&owox).unwrap()[0];
        let st = skill_status(s, repo);
        // 登録可 (draft でない)。
        assert_ne!(
            st.stage,
            Stage::Draft,
            "明示 skill は scripts があっても登録可"
        );
        // warning が出る。
        assert!(
            st.warnings.iter().any(|w| w.contains("tests/")),
            "scripts/ があるのに tests/ が無い旨の warning が必要: {:?}",
            st.warnings
        );
    }

    /// script があるのに SKILL.md で script 名への言及が無い場合は draft 扱いになる。
    #[test]
    fn script_not_mentioned_in_skill_md_is_draft() {
        let owox = tempdir();
        let repo = owox.parent().unwrap();
        // SKILL.md に run.sh への言及なし。
        let fm =
            "---\nname: t\ndescription: d\n---\n\nDo something without mentioning the script.\n";
        make_skill(&owox, "noref", fm, Some("implicit = false\n"), None);
        write_script(&owox, "noref", "run.sh");
        let s = &load_skills(&owox).unwrap()[0];
        let st = skill_status(s, repo);
        // draft 扱い。
        assert_eq!(
            st.stage,
            Stage::Draft,
            "SKILL.md に script 名の言及が無い場合は draft"
        );
        assert!(
            st.warnings.iter().any(|w| w.contains("SKILL.md")),
            "SKILL.md に script 名の言及が無い旨の warning が必要: {:?}",
            st.warnings
        );
    }

    /// tests が script を呼んでいない場合は warning が出る。
    #[test]
    fn tests_not_calling_script_yields_warning() {
        let owox = tempdir();
        let repo = owox.parent().unwrap();
        // SKILL.md は run.sh を参照し、script も存在する。しかし tests は run.sh を呼ばない。
        let fm = "---\nname: t\ndescription: d\n---\n\nRun `scripts/run.sh`.\n";
        make_skill(&owox, "notcalling", fm, Some("implicit = false\n"), None);
        write_script(&owox, "notcalling", "run.sh");
        // tests ディレクトリを作り、run.sh を呼ばないテストを置く。
        let tdir = owox.join("skills/notcalling/tests");
        std::fs::create_dir_all(&tdir).unwrap();
        let tpath = tdir.join("t.sh");
        std::fs::write(&tpath, "#!/bin/sh\nexit 0\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&tpath, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let s = &load_skills(&owox).unwrap()[0];
        let st = skill_status(s, repo);
        // warning が出る (run.sh を呼んでいない)。
        assert!(
            st.warnings.iter().any(|w| w.contains("run.sh")),
            "tests が run.sh を呼んでいない旨の warning が必要: {:?}",
            st.warnings
        );
    }

    /// warnings は skill.list の出力にも含まれる。
    #[test]
    fn list_envelope_includes_warnings_field() {
        let (repo, owox) = repo_and_owox();
        // implicit=false、script あり、tests なし。
        let fm = "---\nname: t\ndescription: d\n---\n\nRun `scripts/run.sh`.\n";
        make_skill(&owox, "withwarn", fm, Some("implicit = false\n"), None);
        write_script(&owox, "withwarn", "run.sh");
        let env = list_skills_envelope(&owox, &repo);
        let data = env.data.unwrap();
        let skill_entry = &data["skills"].as_array().unwrap()[0];
        assert!(
            skill_entry.get("warnings").is_some(),
            "skill.list の各エントリに warnings フィールドが必要"
        );
    }
}
