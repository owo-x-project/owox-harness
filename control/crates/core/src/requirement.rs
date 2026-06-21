//! requirements 正本層: `.owox/requirements/<id>.md` に1要件1ファイル (記録層と同形式)。
//!
//! 状態はライフサイクル (draft / accepted / superseded) のみ持つ。充足 (met) は保存せず、
//! 受け入れ基準の検証 link から verify.run が導出する (スライス2。
//! `docs/decisions/20260614-Phase6-requirements正本層.md`)。
//!
//! 受け入れ基準は rules の不可逆操作と同じ「エントリ + 属性行」方式で書く。
//! 1基準ごとに検証 link を張り、欠落を基準単位で機械検出できる (スライス2 の trace)。
//!
//! ID は日付+slug。中央台帳を持たず並行ブランチで衝突しない (来歴・タスクと同方針)。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde_json::json;

use crate::envelope::Envelope;
use crate::markdown::{Doc, split_pair};
use crate::record::{
    DecisionLinks, DecisionStatus, RecordInput, allocate_id, record_decision, slugify,
    strip_date_prefix,
};

/// 要件のライフサイクル状態。人間が宣言する。充足はここに持たない。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RequirementStatus {
    /// 検討中。まだ確定していない。
    #[default]
    Draft,
    /// 確定・有効。
    Accepted,
    /// 新要件へ置き換えた。
    Superseded,
}

impl RequirementStatus {
    pub fn parse(value: &str) -> Result<RequirementStatus, String> {
        match value.trim() {
            "draft" => Ok(RequirementStatus::Draft),
            "accepted" => Ok(RequirementStatus::Accepted),
            "superseded" => Ok(RequirementStatus::Superseded),
            other => Err(format!(
                "status は draft / accepted / superseded のみ: {other}"
            )),
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            RequirementStatus::Draft => "draft",
            RequirementStatus::Accepted => "accepted",
            RequirementStatus::Superseded => "superseded",
        }
    }
}

/// 要件の種類。任意。起草方法 (PRFAQ/lightweight) や性質軸とは独立で、どの要件にも付けられる。
///
/// 振る舞いか品質特性かを型として残し、検証戦略の目安と将来の owlspec 橋渡しに使う
/// (技術・設計上の制約は要件でなく来歴へ置く。`docs/decisions/20260620-要件分類とPRFAQ正本.md`)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequirementKind {
    /// 機能要件。系がする振る舞い。受け入れ基準のテスト link で検証しやすい。
    Functional,
    /// 非機能要件。性能・安全・可用性などの品質特性。quality.toml の適応度関数や検査委譲で守る側。
    NonFunctional,
}

impl RequirementKind {
    pub fn parse(value: &str) -> Result<RequirementKind, String> {
        match value.trim() {
            "functional" => Ok(RequirementKind::Functional),
            "non-functional" => Ok(RequirementKind::NonFunctional),
            other => Err(format!(
                "kind は functional / non-functional のみ: {other}"
            )),
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            RequirementKind::Functional => "functional",
            RequirementKind::NonFunctional => "non-functional",
        }
    }
}

/// 要件完了 (充足) の判定結果。verify.run が受け入れ基準の検証 link と検査結果から導出する。
///
/// 検証 (verification) は「検査が通った」、met は「要件が満たされた」と語を分ける
/// (完了3区別の意味の違い。`docs/decisions/20260614-Phase6-要件完了の機械判定.md`)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Met {
    /// 全受け入れ基準が実在検査へ link され、その検査が全通過。
    Met,
    /// 機械判定できない (基準なし / 検証 link 欠落 / link 先の検査が config に無い)。
    NeedsHuman,
    /// link 先の検査が落ちた。
    Failed,
}

impl Met {
    pub fn as_str(self) -> &'static str {
        match self {
            Met::Met => "met",
            Met::NeedsHuman => "needs_human",
            Met::Failed => "failed",
        }
    }
}

/// 受け入れ基準 1 件。GIVEN/WHEN/THEN と任意の検証 link を持つ。
///
/// id は要件内で一意な永続番号 (追加時に最大+1、再利用しない)。並べ替え・削除に強く、
/// スライス2 の trace 欠落検出が基準単位で安定する。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcceptanceCriterion {
    pub id: u32,
    /// 短い名前 (任意)。
    pub title: String,
    pub given: String,
    pub when: String,
    pub then: String,
    /// 検証 link (テスト名・検査名)。任意。後から link_verification で張れる。
    pub verify: Option<String>,
}

/// 受け入れ基準の入力 (id は採番されるため持たない)。
#[derive(Debug, Clone, Default)]
pub struct CriterionInput {
    pub given: String,
    pub when: String,
    pub then: String,
    pub verify: Option<String>,
}

/// 要件の link 先。検証 link は基準単位なのでここには持たない。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RequirementLinks {
    /// 関連来歴 (title/statement 変更時に来歴連動で張る)。
    pub decision: Option<String>,
}

impl RequirementLinks {
    fn is_empty(&self) -> bool {
        self.decision.is_none()
    }
}

/// 要件 1 件。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Requirement {
    pub id: String,
    pub title: String,
    pub status: RequirementStatus,
    /// 要件本文 (何を満たすべきか)。
    pub statement: String,
    pub criteria: Vec<AcceptanceCriterion>,
    pub links: RequirementLinks,
    /// 置き換えた旧要件 ID。
    pub supersedes: Vec<String>,
    /// 優先度ランク (理想先行)。小さいほど高優先。人間が並べる・AI は提案まで。
    /// prioritization=ideal-first の時だけ意味を持つ (`docs/decisions/20260618-Phase9-性質軸適応機構.md`)。
    pub priority: Option<u32>,
    /// 層タグ (クリーンアーキ)。architecture=layered の時だけ層別充足報告に使う。
    pub layer: Option<String>,
    /// 段タグ (段階化)。delivery=phased の時だけ stage グルーピングに使う。
    pub stage: Option<String>,
    /// 種類 (機能/非機能)。任意。性質軸と独立でどの要件にも付けられる。
    pub kind: Option<RequirementKind>,
}

/// requirement.create の入力。
#[derive(Debug, Clone, Default)]
pub struct CreateRequirementInput {
    pub title: String,
    pub statement: String,
    pub status: RequirementStatus,
    pub criteria: Vec<CriterionInput>,
    pub supersedes: Vec<String>,
    pub priority: Option<u32>,
    pub layer: Option<String>,
    pub stage: Option<String>,
    pub kind: Option<RequirementKind>,
    /// 便益・なぜ (誰がどう得をするか)。指定時は来歴へ記録し要件へ link する。
    /// requirements-shape=prfaq では起草時に必須 (mcp 側でゲート)。便益・なぜは来歴の領分
    /// (`docs/decisions/20260620-要件分類とPRFAQ正本.md`)。
    pub benefit: Option<String>,
}

/// requirement.update の入力。
#[derive(Debug, Clone, Default)]
pub struct UpdateRequirementInput {
    /// 新しいタイトル (本質変更)。変えるなら reason 必須・来歴へ記録する。
    pub title: Option<String>,
    /// 新しい要件本文 (本質変更)。変えるなら reason 必須・来歴へ記録する。
    pub statement: Option<String>,
    /// 新しい状態。
    pub status: Option<String>,
    /// title / statement 変更の理由。変える時は必須。
    pub reason: Option<String>,
    /// 優先度ランク (理想先行)。設定時のみ変える。軽量変更で来歴連動しない。
    pub priority: Option<u32>,
    /// 層タグ (クリーンアーキ)。設定時のみ変える。
    pub layer: Option<String>,
    /// 段タグ (段階化)。設定時のみ変える。
    pub stage: Option<String>,
    /// 種類 (functional / non-functional)。設定時のみ変える。空文字で消す。
    pub kind: Option<String>,
}

impl Requirement {
    /// Markdown へ描画する。
    fn render(&self) -> String {
        let mut out = format!("# {}\n\n", self.title);
        out.push_str(&format!("## Status\n\n{}\n\n", self.status.as_str()));

        if !self.statement.trim().is_empty() {
            out.push_str(&format!("## Statement\n\n{}\n\n", self.statement.trim()));
        }

        if let Some(p) = self.priority {
            out.push_str(&format!("## Priority\n\n{p}\n\n"));
        }
        if let Some(l) = &self.layer
            && !l.trim().is_empty()
        {
            out.push_str(&format!("## Layer\n\n{}\n\n", l.trim()));
        }
        if let Some(s) = &self.stage
            && !s.trim().is_empty()
        {
            out.push_str(&format!("## Stage\n\n{}\n\n", s.trim()));
        }
        if let Some(k) = self.kind {
            out.push_str(&format!("## Kind\n\n{}\n\n", k.as_str()));
        }

        if !self.criteria.is_empty() {
            out.push_str("## Acceptance criteria\n\n");
            for c in &self.criteria {
                if c.title.trim().is_empty() {
                    out.push_str(&format!("- {}\n", c.id));
                } else {
                    out.push_str(&format!("- {}: {}\n", c.id, c.title.trim()));
                }
                if !c.given.trim().is_empty() {
                    out.push_str(&format!("given: {}\n", c.given.trim()));
                }
                if !c.when.trim().is_empty() {
                    out.push_str(&format!("when: {}\n", c.when.trim()));
                }
                if !c.then.trim().is_empty() {
                    out.push_str(&format!("then: {}\n", c.then.trim()));
                }
                if let Some(v) = &c.verify
                    && !v.trim().is_empty()
                {
                    out.push_str(&format!("verify: {}\n", v.trim()));
                }
            }
            out.push('\n');
        }

        if !self.links.is_empty() {
            out.push_str("## Links\n\n");
            if let Some(d) = &self.links.decision {
                out.push_str(&format!("- decision: {d}\n"));
            }
            out.push('\n');
        }

        if !self.supersedes.is_empty() {
            out.push_str("## Supersedes\n\n");
            for s in &self.supersedes {
                out.push_str(&format!("- {s}\n"));
            }
            out.push('\n');
        }

        out
    }

    /// ファイル本文から読む。title は 1 行目の `# `、他は `## ` 節。
    fn parse(id: &str, text: &str) -> Result<Requirement, String> {
        let title = text
            .lines()
            .map(str::trim)
            .find_map(|l| l.strip_prefix("# ").filter(|_| !l.starts_with("## ")))
            .unwrap_or("")
            .trim()
            .to_string();

        let mut doc = Doc::parse(text);

        let status = doc
            .take("Status")
            .map(|s| s.text())
            .ok_or_else(|| "Status セクションが必須".to_string())
            .and_then(|t| RequirementStatus::parse(&t))?;

        let statement = doc.take("Statement").map(|s| s.text()).unwrap_or_default();

        let priority = match doc.take("Priority").map(|s| s.text()) {
            Some(t) if !t.trim().is_empty() => Some(
                t.trim()
                    .parse::<u32>()
                    .map_err(|_| format!("Priority は整数ランクのみ: {t}"))?,
            ),
            _ => None,
        };
        let layer = doc
            .take("Layer")
            .map(|s| s.text())
            .filter(|t| !t.trim().is_empty());
        let stage = doc
            .take("Stage")
            .map(|s| s.text())
            .filter(|t| !t.trim().is_empty());
        let kind = match doc.take("Kind").map(|s| s.text()) {
            Some(t) if !t.trim().is_empty() => Some(RequirementKind::parse(&t)?),
            _ => None,
        };

        let criteria = match doc.take("Acceptance criteria") {
            Some(section) => parse_criteria(&section)?,
            None => Vec::new(),
        };

        let mut links = RequirementLinks::default();
        if let Some(section) = doc.take("Links") {
            for item in section.list() {
                let (key, value) = split_pair(&item);
                match key.as_str() {
                    "decision" => links.decision = Some(value),
                    other => return Err(format!("Links の未知のキー: {other}")),
                }
            }
        }

        let supersedes = doc.take("Supersedes").map(|s| s.list()).unwrap_or_default();

        let remaining = doc.remaining_headings();
        if !remaining.is_empty() {
            return Err(format!("未知の見出し: {}", remaining.join(", ")));
        }

        Ok(Requirement {
            id: id.to_string(),
            title,
            status,
            statement,
            criteria,
            links,
            supersedes,
            priority,
            layer,
            stage,
            kind,
        })
    }

    /// 構造化した JSON (requirement.get の data)。
    fn to_json(&self) -> serde_json::Value {
        json!({
            "id": self.id,
            "title": self.title,
            "status": self.status.as_str(),
            "statement": self.statement,
            "criteria": self.criteria.iter().map(|c| json!({
                "id": c.id,
                "title": c.title,
                "given": c.given,
                "when": c.when,
                "then": c.then,
                "verify": c.verify,
            })).collect::<Vec<_>>(),
            "links": { "decision": self.links.decision },
            "supersedes": self.supersedes,
            "priority": self.priority,
            "layer": self.layer,
            "stage": self.stage,
            "kind": self.kind.map(RequirementKind::as_str),
        })
    }

    /// 検証 link が欠ける受け入れ基準の数 (スライス2 trace の素地)。
    pub fn unlinked(&self) -> usize {
        self.criteria
            .iter()
            .filter(|c| c.verify.as_deref().unwrap_or("").trim().is_empty())
            .count()
    }

    /// accepted かつ「基準が無い or 検証 link が欠ける」要件は trace が未完成。
    ///
    /// 検査を実行せず静的に分かる信号。next が先回りで出す
    /// (`docs/decisions/20260614-Phase6-要件完了の機械判定.md`)。
    pub fn needs_trace(&self) -> bool {
        self.status == RequirementStatus::Accepted
            && (self.criteria.is_empty() || self.unlinked() > 0)
    }

    /// 要件完了 (met) を判定する。受け入れ基準の検証 link を検査結果へ照合する。
    ///
    /// check_passed は config の検査 name → 通過したか。link 先が表に無ければ実行できない
    /// (runtime dangling)。判定の優先順は unlinked → dangling → failed → met。
    /// reason は機械判定できない/落ちた基準の番号を簡潔に列挙する。
    pub fn judge(&self, check_passed: &BTreeMap<String, bool>) -> (Met, String) {
        if self.criteria.is_empty() {
            return (
                Met::NeedsHuman,
                "no acceptance criteria to judge".to_string(),
            );
        }

        let mut unlinked = Vec::new();
        let mut dangling = Vec::new();
        let mut failed = Vec::new();
        for c in &self.criteria {
            match c.verify.as_deref().map(str::trim).filter(|v| !v.is_empty()) {
                None => unlinked.push(c.id),
                Some(check) => match check_passed.get(check) {
                    None => dangling.push(c.id),
                    Some(true) => {}
                    Some(false) => failed.push(c.id),
                },
            }
        }

        if !unlinked.is_empty() {
            return (
                Met::NeedsHuman,
                format!(
                    "criteria without a verification link: {}",
                    numbers(&unlinked)
                ),
            );
        }
        if !dangling.is_empty() {
            return (
                Met::NeedsHuman,
                format!(
                    "criteria linking to a check not in config.toml: {}",
                    numbers(&dangling)
                ),
            );
        }
        if !failed.is_empty() {
            return (
                Met::Failed,
                format!("criteria whose linked check failed: {}", numbers(&failed)),
            );
        }
        (Met::Met, "all acceptance criteria are verified".to_string())
    }
}

/// 基準番号の並びを `1, 2, 3` の形へ。
fn numbers(ids: &[u32]) -> String {
    ids.iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}

/// 受け入れ基準の節を読む。
///
/// `- <id>: <名前>` がエントリ、その下の `given:` / `when:` / `then:` / `verify:` 行が属性。
/// rules の不可逆操作 (エントリ + `detect:` 属性) と同じ読み方 (`model.rs` の parse_irreversible)。
fn parse_criteria(section: &crate::markdown::Section) -> Result<Vec<AcceptanceCriterion>, String> {
    let mut out: Vec<AcceptanceCriterion> = Vec::new();
    for line in section.lines() {
        if let Some(item) = line.strip_prefix("- ") {
            let (id_str, title) = split_pair(item.trim());
            let id: u32 = id_str
                .parse()
                .map_err(|_| format!("受け入れ基準の id は番号でなければならない: {id_str}"))?;
            out.push(AcceptanceCriterion {
                id,
                title,
                given: String::new(),
                when: String::new(),
                then: String::new(),
                verify: None,
            });
            continue;
        }

        // 箇条書きでない行は直前エントリの属性。
        let (key, value) = split_pair(line);
        let entry = out
            .last_mut()
            .ok_or("受け入れ基準の属性は基準の箇条書きの後に書く")?;
        match key.as_str() {
            "given" => entry.given = value,
            "when" => entry.when = value,
            "then" => entry.then = value,
            "verify" => entry.verify = Some(value).filter(|v| !v.trim().is_empty()),
            other => return Err(format!("受け入れ基準の未知のキー: {other}")),
        }
    }
    Ok(out)
}

/// `.owox/requirements/`。
fn requirements_dir(owox_dir: &Path) -> PathBuf {
    owox_dir.join("requirements")
}

fn requirement_path(owox_dir: &Path, id: &str) -> PathBuf {
    requirements_dir(owox_dir).join(format!("{id}.md"))
}

/// 層ごとの要件進行度 (クリーンアーキの層別充足報告)。architecture=layered の時だけ next が出す。
///
/// 各層の (層名, 要件数, trace 済み数) を層名で整列して返す。trace 済み = accepted かつ trace 完成
/// (受け入れ基準あり・全基準に検証 link)。met (検査通過) でなく trace を軽い代理にする
/// (検査を走らせず静的に出せる。`docs/decisions/20260618-Phase9-性質軸適応機構.md`)。
/// layer 未設定の要件は "(unlayered)" にまとめる。
pub fn layer_progress(reqs: &[Requirement]) -> Vec<(String, usize, usize)> {
    use std::collections::BTreeMap;
    let mut by_layer: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    for r in reqs {
        let key = r.layer.clone().unwrap_or_else(|| "(unlayered)".to_string());
        let entry = by_layer.entry(key).or_default();
        entry.0 += 1;
        if r.status == RequirementStatus::Accepted && !r.needs_trace() {
            entry.1 += 1;
        }
    }
    by_layer
        .into_iter()
        .map(|(k, (total, traced))| (k, total, traced))
        .collect()
}

/// 全要件を読む。`.owox/requirements/*.md`。ディレクトリが無ければ空。
pub fn list_requirements(owox_dir: &Path) -> Result<Vec<Requirement>, String> {
    let dir = requirements_dir(owox_dir);
    let read = match std::fs::read_dir(&dir) {
        Ok(read) => read,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(format!("{} を読めない: {err}", dir.display())),
    };

    let mut requirements = Vec::new();
    for entry in read {
        let path = entry.map_err(|e| e.to_string())?.path();
        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        let id = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or_default()
            .to_string();
        let text = std::fs::read_to_string(&path)
            .map_err(|e| format!("{} を読めない: {e}", path.display()))?;
        let requirement = Requirement::parse(&id, &text)
            .map_err(|e| format!("{} を解釈できない: {e}", path.display()))?;
        requirements.push(requirement);
    }
    requirements.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(requirements)
}

/// 既存要件を読む。無ければ Err 文言。
fn load_requirement(owox_dir: &Path, id: &str) -> Result<Requirement, String> {
    let path = requirement_path(owox_dir, id);
    match std::fs::read_to_string(&path) {
        Ok(text) => Requirement::parse(id, &text),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Err(format!("要件が無い: {id}")),
        Err(err) => Err(format!("{} を読めない: {err}", path.display())),
    }
}

fn save_requirement(owox_dir: &Path, requirement: &Requirement) -> Result<(), String> {
    std::fs::write(
        requirement_path(owox_dir, &requirement.id),
        requirement.render(),
    )
    .map_err(|e| format!("要件を書けない: {e}"))
}

/// 検証 link が既知の検査名か照合する。未知なら利用可能な検査名を添えた Err 文言。
///
/// dangling link (実行できない link) を書き込み時に発生不能にする
/// (task.create の依存先照合と同型。`docs/decisions/20260614-Phase6-要件完了の機械判定.md`)。
fn check_known(verify: &str, known_checks: &[String]) -> Result<(), String> {
    let v = verify.trim();
    if v.is_empty() || known_checks.iter().any(|k| k == v) {
        return Ok(());
    }
    let available = if known_checks.is_empty() {
        "none are configured".to_string()
    } else {
        known_checks.join(", ")
    };
    Err(format!(
        "Unknown check: {v}. Link an existing check (configured: {available}) or add it to [[verify.checks]] in config.toml first."
    ))
}

/// requirement.create。`.owox/requirements/<id>.md` を書く。
///
/// inline の検証 link は known_checks へ照合する (未知 link を弾く)。
pub fn create_requirement(
    owox_dir: &Path,
    today: &str,
    known_checks: &[String],
    known_layers: &[String],
    input: CreateRequirementInput,
) -> Envelope {
    if input.title.trim().is_empty() {
        return Envelope::failed("title が空");
    }
    for c in &input.criteria {
        if let Some(v) = &c.verify
            && let Err(err) = check_known(v, known_checks)
        {
            return Envelope::failed(err);
        }
    }
    if let Some(l) = &input.layer
        && let Err(err) = crate::quality::check_known_layer(l, known_layers)
    {
        return Envelope::failed(err);
    }
    let dir = requirements_dir(owox_dir);
    if let Err(err) = std::fs::create_dir_all(&dir) {
        return Envelope::failed(format!("{} を作れない: {err}", dir.display()));
    }

    // 入力の受け入れ基準へ 1 から採番する。
    let criteria = input
        .criteria
        .into_iter()
        .enumerate()
        .map(|(i, c)| AcceptanceCriterion {
            id: (i as u32) + 1,
            title: String::new(),
            given: c.given,
            when: c.when,
            then: c.then,
            verify: c.verify.filter(|v| !v.trim().is_empty()),
        })
        .collect();

    let id = allocate_id(&dir, today, &slugify(&input.title));
    let mut requirement = Requirement {
        id: id.clone(),
        title: input.title,
        status: input.status,
        statement: input.statement,
        criteria,
        links: RequirementLinks::default(),
        supersedes: input.supersedes,
        priority: input.priority,
        layer: input.layer.filter(|s| !s.trim().is_empty()),
        stage: input.stage.filter(|s| !s.trim().is_empty()),
        kind: input.kind,
    };

    if let Err(err) = save_requirement(owox_dir, &requirement) {
        return Envelope::failed(err);
    }

    // 便益・なぜは要件本文でなく来歴の領分。指定時は採用済み decision として残し要件へ link する
    // (PRFAQ の逆算の蒸留を行動の地点で固定する。`docs/decisions/20260620-要件分類とPRFAQ正本.md`)。
    let mut decision_ids = Vec::new();
    if let Some(benefit) = input.benefit.as_deref().map(str::trim).filter(|b| !b.is_empty()) {
        let record = record_decision(
            owox_dir,
            today,
            RecordInput {
                title: format!("Benefit of requirement {}", strip_date_prefix(&id)),
                status: DecisionStatus::Adopted,
                rationale: benefit.to_string(),
                links: DecisionLinks {
                    requirement: Some(id.clone()),
                    ..DecisionLinks::default()
                },
                supersedes: Vec::new(),
            },
        );
        requirement.links.decision = record
            .data
            .as_ref()
            .and_then(|d| d["id"].as_str())
            .map(String::from);
        decision_ids = record.decision_ids;
        if let Err(err) = save_requirement(owox_dir, &requirement) {
            return Envelope::failed(err);
        }
    }

    Envelope::ok("Created the requirement.", json!({ "id": id })).with_decision_ids(decision_ids)
}

/// requirement.list。状態で絞り、要約を返す。
pub fn list_requirements_envelope(owox_dir: &Path, status: Option<&str>) -> Envelope {
    let requirements = match list_requirements(owox_dir) {
        Ok(r) => r,
        Err(err) => return Envelope::failed(err),
    };
    let status_filter = match status.map(RequirementStatus::parse).transpose() {
        Ok(s) => s,
        Err(err) => return Envelope::failed(err),
    };

    let listed: Vec<_> = requirements
        .iter()
        .filter(|r| status_filter.is_none_or(|s| r.status == s))
        .map(|r| {
            json!({
                "id": r.id,
                "title": r.title,
                "status": r.status.as_str(),
                "criteria": r.criteria.len(),
                "unlinked": r.unlinked(),
            })
        })
        .collect();

    Envelope::ok(
        format!("{} requirement(s).", listed.len()),
        json!({ "requirements": listed }),
    )
}

/// requirement.get。要件 1 件の全文を構造化して返す (canon 直読み禁止の読み口)。
pub fn get_requirement(owox_dir: &Path, id: &str) -> Envelope {
    match load_requirement(owox_dir, id) {
        Ok(r) => Envelope::ok("Requirement.", r.to_json()),
        Err(err) => Envelope::failed(err),
    }
}

/// requirement.update。status は軽量、title / statement の変更は本質変更で reason 必須・来歴連動。
///
/// 要件本文の本質変更は将来作業が黙って覆してはいけない判断のため来歴へ残す
/// (task.update の title 変更が前例。`docs/decisions/20260614-Phase6-requirements正本層.md`)。
pub fn update_requirement(
    owox_dir: &Path,
    today: &str,
    id: &str,
    known_layers: &[String],
    input: UpdateRequirementInput,
) -> Envelope {
    let mut requirement = match load_requirement(owox_dir, id) {
        Ok(r) => r,
        Err(err) => return Envelope::failed(err),
    };

    if let Some(l) = &input.layer
        && let Err(err) = crate::quality::check_known_layer(l, known_layers)
    {
        return Envelope::failed(err);
    }

    // 種類の更新を早期に解く (未知値で部分書き込みしない)。None=据え置き・Some(None)=消す・Some(Some)=設定。
    let kind_update: Option<Option<RequirementKind>> = match &input.kind {
        None => None,
        Some(s) if s.trim().is_empty() => Some(None),
        Some(s) => match RequirementKind::parse(s) {
            Ok(k) => Some(Some(k)),
            Err(err) => return Envelope::failed(err),
        },
    };

    if let Some(s) = &input.status {
        match RequirementStatus::parse(s) {
            Ok(parsed) => requirement.status = parsed,
            Err(err) => return Envelope::failed(err),
        }
    }

    // title / statement の本質変更を集める。
    let new_title = input
        .title
        .as_deref()
        .map(str::trim)
        .filter(|t| !t.is_empty() && *t != requirement.title);
    let new_statement = input
        .statement
        .as_deref()
        .map(str::trim)
        .filter(|t| *t != requirement.statement.trim());

    let mut decision_ids = Vec::new();
    if new_title.is_some() || new_statement.is_some() {
        let reason = input.reason.as_deref().unwrap_or("").trim();
        if reason.is_empty() {
            return Envelope::failed(
                "Changing a requirement's title or statement is a content change; provide a reason (it is recorded as a decision).",
            );
        }
        let what = match (new_title, new_statement.is_some()) {
            (Some(t), true) => format!("title → {t}, and the statement"),
            (Some(t), false) => format!("title → {t}"),
            (None, _) => "the statement".to_string(),
        };
        let record = record_decision(
            owox_dir,
            today,
            RecordInput {
                title: format!("Update requirement {}: {what}", strip_date_prefix(id)),
                status: DecisionStatus::Adopted,
                rationale: reason.to_string(),
                links: DecisionLinks {
                    requirement: Some(id.to_string()),
                    ..DecisionLinks::default()
                },
                supersedes: Vec::new(),
            },
        );
        requirement.links.decision = record
            .data
            .as_ref()
            .and_then(|d| d["id"].as_str())
            .map(String::from);
        decision_ids = record.decision_ids;

        if let Some(t) = new_title {
            requirement.title = t.to_string();
        }
        if let Some(s) = new_statement {
            requirement.statement = s.to_string();
        }
    }

    // 属性 (優先度/層/段) は軽量変更で来歴連動しない (理想先行の優先順位は人間が並べる行為が正)。
    if input.priority.is_some() {
        requirement.priority = input.priority;
    }
    if let Some(l) = input.layer {
        let l = l.trim();
        requirement.layer = (!l.is_empty()).then(|| l.to_string());
    }
    if let Some(s) = input.stage {
        let s = s.trim();
        requirement.stage = (!s.is_empty()).then(|| s.to_string());
    }
    if let Some(k) = kind_update {
        requirement.kind = k;
    }

    if let Err(err) = save_requirement(owox_dir, &requirement) {
        return Envelope::failed(err);
    }
    Envelope::ok("Updated the requirement.", json!({ "id": id })).with_decision_ids(decision_ids)
}

/// requirement.add_criterion。受け入れ基準を 1 件足す。次の番号を採番する。
pub fn add_criterion(owox_dir: &Path, id: &str, given: &str, when: &str, then: &str) -> Envelope {
    let mut requirement = match load_requirement(owox_dir, id) {
        Ok(r) => r,
        Err(err) => return Envelope::failed(err),
    };
    if given.trim().is_empty() && when.trim().is_empty() && then.trim().is_empty() {
        return Envelope::failed("受け入れ基準は given / when / then のいずれかが必要");
    }

    let next = requirement.criteria.iter().map(|c| c.id).max().unwrap_or(0) + 1;
    requirement.criteria.push(AcceptanceCriterion {
        id: next,
        title: String::new(),
        given: given.trim().to_string(),
        when: when.trim().to_string(),
        then: then.trim().to_string(),
        verify: None,
    });
    if let Err(err) = save_requirement(owox_dir, &requirement) {
        return Envelope::failed(err);
    }
    Envelope::ok(
        "Added an acceptance criterion.",
        json!({ "id": id, "criterion": next }),
    )
}

/// requirement.link_verification。既存の受け入れ基準に検証 link を張る。
///
/// 検証 link は known_checks へ照合する (未知 link を弾き dangling を作らせない)。
pub fn link_verification(
    owox_dir: &Path,
    known_checks: &[String],
    id: &str,
    criterion: u32,
    verification: &str,
) -> Envelope {
    if verification.trim().is_empty() {
        return Envelope::failed("検証 link が空");
    }
    if let Err(err) = check_known(verification, known_checks) {
        return Envelope::failed(err);
    }
    let mut requirement = match load_requirement(owox_dir, id) {
        Ok(r) => r,
        Err(err) => return Envelope::failed(err),
    };
    let Some(target) = requirement.criteria.iter_mut().find(|c| c.id == criterion) else {
        return Envelope::failed(format!("受け入れ基準が無い: {criterion}"));
    };
    target.verify = Some(verification.trim().to_string());
    if let Err(err) = save_requirement(owox_dir, &requirement) {
        return Envelope::failed(err);
    }
    Envelope::ok(
        "Linked verification to the acceptance criterion.",
        json!({ "id": id, "criterion": criterion }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::envelope::Status;

    fn tempdir() -> PathBuf {
        use std::sync::atomic::{AtomicU32, Ordering};
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let pid = std::process::id();
        let dir = std::env::temp_dir().join(format!("owox-req-test-{pid}-{n}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// 検証で使う既知検査名 (照合用)。
    fn known(names: &[&str]) -> Vec<String> {
        names.iter().map(|s| s.to_string()).collect()
    }

    /// 通過した検査の表。
    fn passed(entries: &[(&str, bool)]) -> BTreeMap<String, bool> {
        entries.iter().map(|(n, p)| (n.to_string(), *p)).collect()
    }

    fn create(dir: &Path, title: &str) -> String {
        let env = create_requirement(
            dir,
            "20260614",
            &[],
            &[],
            CreateRequirementInput {
                title: title.to_string(),
                statement: "must do the thing".to_string(),
                ..CreateRequirementInput::default()
            },
        );
        env.data.unwrap()["id"].as_str().unwrap().to_string()
    }

    #[test]
    fn create_then_roundtrip() {
        let dir = tempdir();
        let id = create(&dir, "Login redirect");
        assert_eq!(id, "20260614-Login-redirect");
        let reqs = list_requirements(&dir).unwrap();
        assert_eq!(reqs.len(), 1);
        assert_eq!(reqs[0].title, "Login redirect");
        assert_eq!(reqs[0].status, RequirementStatus::Draft);
        assert_eq!(reqs[0].statement, "must do the thing");
    }

    #[test]
    fn priority_layer_stage_roundtrip() {
        let dir = tempdir();
        let env = create_requirement(
            &dir,
            "20260618",
            &[],
            &[],
            CreateRequirementInput {
                title: "Core rule".to_string(),
                statement: "s".to_string(),
                priority: Some(2),
                layer: Some("core".to_string()),
                stage: Some("mvp".to_string()),
                ..CreateRequirementInput::default()
            },
        );
        let id = env.data.unwrap()["id"].as_str().unwrap().to_string();
        let r = load_requirement(&dir, &id).unwrap();
        assert_eq!(r.priority, Some(2));
        assert_eq!(r.layer.as_deref(), Some("core"));
        assert_eq!(r.stage.as_deref(), Some("mvp"));
    }

    #[test]
    fn create_with_benefit_records_linked_decision() {
        let dir = tempdir();
        let env = create_requirement(
            &dir,
            "20260620",
            &[],
            &[],
            CreateRequirementInput {
                title: "History list".to_string(),
                statement: "s".to_string(),
                benefit: Some("operators recover faster by reusing prior inputs".to_string()),
                ..CreateRequirementInput::default()
            },
        );
        assert_eq!(env.decision_ids.len(), 1);
        let id = env.data.unwrap()["id"].as_str().unwrap().to_string();
        // 要件へ便益 decision が link される。
        let r = load_requirement(&dir, &id).unwrap();
        assert!(r.links.decision.is_some());
        // 便益・なぜは来歴 (decisions) に採用済みで残る。
        let decisions = crate::record::list_decisions(&dir).unwrap();
        assert_eq!(decisions.len(), 1);
        assert!(decisions[0].rationale.contains("recover faster"));
        assert_eq!(decisions[0].links.requirement.as_deref(), Some(id.as_str()));
    }

    #[test]
    fn create_without_benefit_records_no_decision() {
        let dir = tempdir();
        let id = create(&dir, "No benefit");
        let r = load_requirement(&dir, &id).unwrap();
        assert!(r.links.decision.is_none());
        assert!(crate::record::list_decisions(&dir).unwrap().is_empty());
    }

    #[test]
    fn kind_roundtrips_and_rejects_unknown() {
        assert!(RequirementKind::parse("bogus").is_err());
        let dir = tempdir();
        // 機能/非機能を作って読み直しても等価。
        let env = create_requirement(
            &dir,
            "20260620",
            &[],
            &[],
            CreateRequirementInput {
                title: "Latency budget".to_string(),
                statement: "s".to_string(),
                kind: Some(RequirementKind::NonFunctional),
                ..CreateRequirementInput::default()
            },
        );
        let id = env.data.unwrap()["id"].as_str().unwrap().to_string();
        let r = load_requirement(&dir, &id).unwrap();
        assert_eq!(r.kind, Some(RequirementKind::NonFunctional));
        // get の JSON に種類が出る。
        let got = get_requirement(&dir, &id);
        assert_eq!(got.data.unwrap()["kind"], "non-functional");
    }

    #[test]
    fn update_sets_and_clears_kind() {
        let dir = tempdir();
        let id = create(&dir, "X");
        // 設定。
        update_requirement(
            &dir,
            "20260620",
            &id,
            &[],
            UpdateRequirementInput {
                kind: Some("functional".to_string()),
                ..UpdateRequirementInput::default()
            },
        );
        assert_eq!(
            load_requirement(&dir, &id).unwrap().kind,
            Some(RequirementKind::Functional)
        );
        // 未知値は弾く (据え置き)。
        let env = update_requirement(
            &dir,
            "20260620",
            &id,
            &[],
            UpdateRequirementInput {
                kind: Some("bogus".to_string()),
                ..UpdateRequirementInput::default()
            },
        );
        assert_eq!(env.status, Status::Failed);
        assert_eq!(
            load_requirement(&dir, &id).unwrap().kind,
            Some(RequirementKind::Functional)
        );
        // 空文字で消す。
        update_requirement(
            &dir,
            "20260620",
            &id,
            &[],
            UpdateRequirementInput {
                kind: Some("".to_string()),
                ..UpdateRequirementInput::default()
            },
        );
        assert_eq!(load_requirement(&dir, &id).unwrap().kind, None);
    }

    #[test]
    fn layer_tag_validates_against_declared_layers() {
        let dir = tempdir();
        // 層名が宣言されている時、未知 layer は弾く。
        let env = create_requirement(
            &dir,
            "20260618",
            &[],
            &known(&["core", "infra"]),
            CreateRequirementInput {
                title: "x".to_string(),
                statement: "s".to_string(),
                layer: Some("ghost".to_string()),
                ..CreateRequirementInput::default()
            },
        );
        assert_eq!(env.status, Status::Failed);
        assert!(list_requirements(&dir).unwrap().is_empty());
        // 宣言済の層名なら通る。
        let env = create_requirement(
            &dir,
            "20260618",
            &[],
            &known(&["core", "infra"]),
            CreateRequirementInput {
                title: "y".to_string(),
                statement: "s".to_string(),
                layer: Some("core".to_string()),
                ..CreateRequirementInput::default()
            },
        );
        assert_eq!(env.status, Status::Ok);
    }

    #[test]
    fn layer_tag_unconstrained_when_no_layers_declared() {
        let dir = tempdir();
        // 層名が未宣言 (空) なら任意 layer を許す (後方互換・任意宣言)。
        let env = create_requirement(
            &dir,
            "20260618",
            &[],
            &[],
            CreateRequirementInput {
                title: "z".to_string(),
                statement: "s".to_string(),
                layer: Some("anything".to_string()),
                ..CreateRequirementInput::default()
            },
        );
        assert_eq!(env.status, Status::Ok);
    }

    #[test]
    fn update_sets_and_clears_attributes() {
        let dir = tempdir();
        let id = create(&dir, "X");
        update_requirement(
            &dir,
            "20260618",
            &id,
            &[],
            UpdateRequirementInput {
                priority: Some(1),
                layer: Some("infra".to_string()),
                ..UpdateRequirementInput::default()
            },
        );
        let r = load_requirement(&dir, &id).unwrap();
        assert_eq!(r.priority, Some(1));
        assert_eq!(r.layer.as_deref(), Some("infra"));
        // 空文字で層を消す。
        update_requirement(
            &dir,
            "20260618",
            &id,
            &[],
            UpdateRequirementInput {
                layer: Some("".to_string()),
                ..UpdateRequirementInput::default()
            },
        );
        assert_eq!(load_requirement(&dir, &id).unwrap().layer, None);
    }

    #[test]
    fn layer_progress_groups_by_layer() {
        let mut traced = req_with(vec![crit(1, Some("check"))]);
        traced.layer = Some("core".to_string());
        let mut untraced = req_with(vec![crit(1, None)]);
        untraced.layer = Some("core".to_string());
        let mut other = req_with(vec![crit(1, Some("check"))]);
        other.layer = None;
        let p = layer_progress(&[traced, untraced, other]);
        // core: 2 件中 1 件 trace 済み・(unlayered): 1 件中 1 件。
        assert_eq!(p.iter().find(|(k, ..)| k == "core"), Some(&("core".to_string(), 2, 1)));
        assert_eq!(
            p.iter().find(|(k, ..)| k == "(unlayered)"),
            Some(&("(unlayered)".to_string(), 1, 1))
        );
    }

    #[test]
    fn create_with_criteria_assigns_ids() {
        let dir = tempdir();
        let env = create_requirement(
            &dir,
            "20260614",
            &known(&["test_one"]),
            &[],
            CreateRequirementInput {
                title: "feature".to_string(),
                criteria: vec![
                    CriterionInput {
                        given: "a".to_string(),
                        when: "b".to_string(),
                        then: "c".to_string(),
                        verify: Some("test_one".to_string()),
                    },
                    CriterionInput {
                        given: "d".to_string(),
                        when: "e".to_string(),
                        then: "f".to_string(),
                        verify: None,
                    },
                ],
                ..CreateRequirementInput::default()
            },
        );
        let id = env.data.unwrap()["id"].as_str().unwrap().to_string();
        let req = load_requirement(&dir, &id).unwrap();
        assert_eq!(req.criteria.len(), 2);
        assert_eq!(req.criteria[0].id, 1);
        assert_eq!(req.criteria[1].id, 2);
        assert_eq!(req.criteria[0].verify.as_deref(), Some("test_one"));
        assert_eq!(req.criteria[1].verify, None);
        // 検証 link が欠ける基準は 1 件。
        assert_eq!(req.unlinked(), 1);
    }

    #[test]
    fn add_criterion_allocates_next_number() {
        let dir = tempdir();
        let id = create(&dir, "x");
        let env = add_criterion(&dir, &id, "g", "w", "t");
        assert_eq!(env.status, Status::Ok);
        assert_eq!(env.data.unwrap()["criterion"], 1);
        let env = add_criterion(&dir, &id, "g2", "w2", "t2");
        assert_eq!(env.data.unwrap()["criterion"], 2);
        let req = load_requirement(&dir, &id).unwrap();
        assert_eq!(req.criteria.len(), 2);
    }

    #[test]
    fn link_verification_sets_verify_on_criterion() {
        let dir = tempdir();
        let id = create(&dir, "x");
        add_criterion(&dir, &id, "g", "w", "t");
        let k = known(&["test_x"]);
        // 存在しない基準は失敗。
        assert_eq!(
            link_verification(&dir, &k, &id, 99, "test_x").status,
            Status::Failed
        );
        let env = link_verification(&dir, &k, &id, 1, "test_x");
        assert_eq!(env.status, Status::Ok);
        let req = load_requirement(&dir, &id).unwrap();
        assert_eq!(req.criteria[0].verify.as_deref(), Some("test_x"));
        assert_eq!(req.unlinked(), 0);
    }

    #[test]
    fn link_verification_rejects_unknown_check() {
        let dir = tempdir();
        let id = create(&dir, "x");
        add_criterion(&dir, &id, "g", "w", "t");
        // 既知でない検査名への link は弾く (dangling を作らせない)。
        let env = link_verification(&dir, &known(&["real_check"]), &id, 1, "ghost_check");
        assert_eq!(env.status, Status::Failed);
        assert!(env.reason.contains("Unknown check"), "{}", env.reason);
        // 書き込まれていない。
        assert_eq!(
            load_requirement(&dir, &id).unwrap().criteria[0].verify,
            None
        );
    }

    #[test]
    fn create_rejects_unknown_inline_verify() {
        let dir = tempdir();
        let env = create_requirement(
            &dir,
            "20260614",
            &known(&["real_check"]),
            &[],
            CreateRequirementInput {
                title: "feature".to_string(),
                criteria: vec![CriterionInput {
                    given: "a".to_string(),
                    when: "b".to_string(),
                    then: "c".to_string(),
                    verify: Some("ghost_check".to_string()),
                }],
                ..CreateRequirementInput::default()
            },
        );
        assert_eq!(env.status, Status::Failed);
        assert!(list_requirements(&dir).unwrap().is_empty());
    }

    #[test]
    fn status_change_is_lightweight() {
        let dir = tempdir();
        let id = create(&dir, "x");
        let env = update_requirement(
            &dir,
            "20260614",
            &id,
            &[],
            UpdateRequirementInput {
                status: Some("accepted".to_string()),
                ..UpdateRequirementInput::default()
            },
        );
        assert_eq!(env.status, Status::Ok);
        // 状態遷移は来歴を作らない。
        assert!(env.decision_ids.is_empty());
        assert_eq!(
            load_requirement(&dir, &id).unwrap().status,
            RequirementStatus::Accepted
        );
    }

    #[test]
    fn title_or_statement_change_requires_reason_and_records_decision() {
        let dir = tempdir();
        let id = create(&dir, "old title");
        // 理由なしの本質変更は弾く。
        let env = update_requirement(
            &dir,
            "20260614",
            &id,
            &[],
            UpdateRequirementInput {
                statement: Some("a new statement".to_string()),
                ..UpdateRequirementInput::default()
            },
        );
        assert_eq!(env.status, Status::Failed);
        // 理由つきなら通り、来歴へ残り、要件へ decision link が張られる。
        let env = update_requirement(
            &dir,
            "20260614",
            &id,
            &[],
            UpdateRequirementInput {
                title: Some("new title".to_string()),
                statement: Some("a new statement".to_string()),
                reason: Some("scope clarified".to_string()),
                ..UpdateRequirementInput::default()
            },
        );
        assert_eq!(env.status, Status::Ok);
        assert_eq!(env.decision_ids.len(), 1);
        let req = load_requirement(&dir, &id).unwrap();
        assert_eq!(req.title, "new title");
        assert_eq!(req.statement, "a new statement");
        assert!(req.links.decision.is_some());
        // 来歴が要件へ link されている。
        let decisions = crate::record::list_decisions(&dir).unwrap();
        let d = decisions
            .iter()
            .find(|d| d.rationale.contains("scope clarified"))
            .unwrap();
        assert_eq!(d.links.requirement.as_deref(), Some(id.as_str()));
    }

    #[test]
    fn criteria_roundtrip_through_markdown() {
        let dir = tempdir();
        let env = create_requirement(
            &dir,
            "20260614",
            &known(&["test_redirect"]),
            &[],
            CreateRequirementInput {
                title: "feature".to_string(),
                statement: "body".to_string(),
                criteria: vec![CriterionInput {
                    given: "logged out".to_string(),
                    when: "open dashboard".to_string(),
                    then: "redirect to login".to_string(),
                    verify: Some("test_redirect".to_string()),
                }],
                supersedes: vec!["20260613-old".to_string()],
                ..CreateRequirementInput::default()
            },
        );
        let id = env.data.unwrap()["id"].as_str().unwrap().to_string();
        // ファイルから読み直しても等価。
        let req = load_requirement(&dir, &id).unwrap();
        assert_eq!(req.criteria[0].given, "logged out");
        assert_eq!(req.criteria[0].when, "open dashboard");
        assert_eq!(req.criteria[0].then, "redirect to login");
        assert_eq!(req.criteria[0].verify.as_deref(), Some("test_redirect"));
        assert_eq!(req.supersedes, vec!["20260613-old".to_string()]);
    }

    #[test]
    fn unknown_status_is_rejected() {
        assert!(RequirementStatus::parse("bogus").is_err());
        let dir = tempdir();
        let id = create(&dir, "x");
        let env = update_requirement(
            &dir,
            "20260614",
            &id,
            &[],
            UpdateRequirementInput {
                status: Some("bogus".to_string()),
                ..UpdateRequirementInput::default()
            },
        );
        assert_eq!(env.status, Status::Failed);
    }

    /// 検査結果から要件 1 件を作る (judge のテスト用)。
    fn req_with(criteria: Vec<AcceptanceCriterion>) -> Requirement {
        Requirement {
            id: "20260614-r".to_string(),
            title: "r".to_string(),
            status: RequirementStatus::Accepted,
            statement: String::new(),
            criteria,
            links: RequirementLinks::default(),
            supersedes: Vec::new(),
            priority: None,
            layer: None,
            stage: None,
            kind: None,
        }
    }

    fn crit(id: u32, verify: Option<&str>) -> AcceptanceCriterion {
        AcceptanceCriterion {
            id,
            title: String::new(),
            given: "g".to_string(),
            when: "w".to_string(),
            then: "t".to_string(),
            verify: verify.map(str::to_string),
        }
    }

    #[test]
    fn judge_no_criteria_needs_human() {
        let (met, _) = req_with(vec![]).judge(&passed(&[]));
        assert_eq!(met, Met::NeedsHuman);
    }

    #[test]
    fn judge_unlinked_needs_human() {
        let r = req_with(vec![crit(1, Some("c")), crit(2, None)]);
        let (met, reason) = r.judge(&passed(&[("c", true)]));
        assert_eq!(met, Met::NeedsHuman);
        assert!(reason.contains('2'), "{reason}");
    }

    #[test]
    fn judge_dangling_needs_human() {
        // link 先が検査結果に無い (config から消えた端ケース)。
        let r = req_with(vec![crit(1, Some("ghost"))]);
        let (met, _) = r.judge(&passed(&[("real", true)]));
        assert_eq!(met, Met::NeedsHuman);
    }

    #[test]
    fn judge_failed_check_is_failed() {
        let r = req_with(vec![crit(1, Some("c"))]);
        let (met, reason) = r.judge(&passed(&[("c", false)]));
        assert_eq!(met, Met::Failed);
        assert!(reason.contains('1'), "{reason}");
    }

    #[test]
    fn judge_all_verified_is_met() {
        let r = req_with(vec![crit(1, Some("c")), crit(2, Some("d"))]);
        let (met, _) = r.judge(&passed(&[("c", true), ("d", true)]));
        assert_eq!(met, Met::Met);
    }

    #[test]
    fn needs_trace_only_for_accepted_with_gaps() {
        // accepted で基準なし → trace 要。
        let mut r = req_with(vec![]);
        assert!(r.needs_trace());
        // 全 link 済み → trace 不要。
        r.criteria = vec![crit(1, Some("c"))];
        assert!(!r.needs_trace());
        // draft なら判定外。
        r.status = RequirementStatus::Draft;
        r.criteria = vec![];
        assert!(!r.needs_trace());
    }

    #[test]
    fn unknown_heading_is_rejected() {
        let dir = tempdir();
        let dir2 = requirements_dir(&dir);
        std::fs::create_dir_all(&dir2).unwrap();
        std::fs::write(
            dir2.join("20260614-bad.md"),
            "# bad\n\n## Status\n\ndraft\n\n## Bogus\n\n- x\n",
        )
        .unwrap();
        let err = list_requirements(&dir).unwrap_err();
        assert!(err.contains("未知の見出し"), "{err}");
    }
}
