//! 頻出手順の検知 (`docs/decisions/20260616-Phase8-パターンからスキル育成.md`)。
//!
//! 使用履歴 (`usage.rs`) を読み、隣接する name の列 (長さ 2..=max_len) の出現回数を数える。
//! 閾値超え かつ 未スキル化 の列を「育てられる手順」として助言で返す (advisory・commit を止めない)。
//! 雛形は自動生成しない。提案 = 始端で、技の固定化は既存ライフサイクル (テスト合格 + 人間昇格) が担う。
//!
//! decay と同じく run_* 別関数で既存署名を変えない。読みは usage.log のみ (床へ注入しない)。

use std::collections::BTreeMap;
use std::path::Path;

use crate::quality::RoutineConfig;
use crate::skill::Skill;
use crate::usage;

/// 提案種別。script に落としやすいものだけ `ScriptSkill` へ上げる。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoutineKind {
    Skill,
    ScriptSkill,
}

impl RoutineKind {
    pub fn as_str(self) -> &'static str {
        match self {
            RoutineKind::Skill => "skill",
            RoutineKind::ScriptSkill => "script-skill",
        }
    }
}

/// script-skill 提案の確信度 (機械式の3段階)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Confidence {
    /// 必須条件すべて充足 かつ 強い判定 signal ≥2 かつ 出現回数 ≥ 2×閾値。
    High,
    /// 必須条件すべて充足 かつ 強い判定 signal ≥1。
    Medium,
    /// script-skill にならない、または弱い。
    Low,
}

impl Confidence {
    pub fn as_str(self) -> &'static str {
        match self {
            Confidence::High => "high",
            Confidence::Medium => "medium",
            Confidence::Low => "low",
        }
    }
}

/// 育てられる手順の提案 1 件。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoutineSuggestion {
    /// 隣接して繰り返される name の列 (入口コマンド / シェル操作)。
    pub sequence: Vec<String>,
    /// 出現回数。
    pub occurrences: usize,
    /// 通常 skill か、script に寄せやすいか。
    pub kind: RoutineKind,
    /// 判定理由。
    pub reasons: Vec<String>,
    /// script-skill 候補の保存先の叩き台。
    pub suggested_script: Option<String>,
    /// 最低限の試験の置き場所の叩き台 (条件6: 小検査データで試せるか は機械検査不能。
    /// test_hint として人間判断に委ねる)。
    pub test_hint: Option<String>,
    /// 提案の確信度 (機械式3段階)。
    pub confidence: Confidence,
}

/// 使用履歴から頻出手順を検知する。閾値超え かつ 未スキル化 の列を返す。
///
/// 長い列を優先し、既に採られた長い列に含まれる短い部分列は落とす (重複提案を減らす)。
pub fn run_routine_suggestions(
    owox_dir: &Path,
    config: &RoutineConfig,
    skills: &[Skill],
) -> Vec<RoutineSuggestion> {
    let names = usage::read_names(owox_dir);
    if names.len() < 2 || config.max_len < 2 {
        return Vec::new();
    }

    // 長さごとに隣接 n-gram の出現回数を数える。
    let mut counts: BTreeMap<Vec<String>, usize> = BTreeMap::new();
    let max_len = config.max_len.min(names.len());
    for len in 2..=max_len {
        for window in names.windows(len) {
            *counts.entry(window.to_vec()).or_default() += 1;
        }
    }

    // 閾値超え かつ 2 種以上の name を含む候補を長さ降順・出現回数降順で並べる。
    // 同一 name の連続 (例 Bash の連打) は「手順」でなく雑音なので除く (distinct >= 2)。
    let mut candidates: Vec<RoutineSuggestion> = counts
        .into_iter()
        .filter(|(seq, n)| *n as u32 >= config.min_occurrences && has_distinct_steps(seq))
        .map(|(sequence, occurrences)| {
            build_suggestion(sequence, occurrences, config.min_occurrences)
        })
        .collect();
    candidates.sort_by(|a, b| {
        b.sequence
            .len()
            .cmp(&a.sequence.len())
            .then(b.occurrences.cmp(&a.occurrences))
            .then(a.sequence.cmp(&b.sequence))
    });

    let mut kept: Vec<RoutineSuggestion> = Vec::new();
    for c in candidates {
        // 既に採られた長い列に含まれる部分列は落とす。
        if kept
            .iter()
            .any(|k| is_subsequence(&c.sequence, &k.sequence))
        {
            continue;
        }
        // 未スキル化のものだけ提案する (既存スキルを再提案しない)。
        if is_skilled(&c.sequence, skills) {
            continue;
        }
        kept.push(c);
    }
    kept
}

fn build_suggestion(
    sequence: Vec<String>,
    occurrences: usize,
    min_occurrences: u32,
) -> RoutineSuggestion {
    let mut reasons = vec![format!("repeated {occurrences} times")];
    let scriptable = is_script_skill_candidate(&sequence, &mut reasons);
    let slug = slugify_seq(&sequence);
    let strong_signals = sequence
        .iter()
        .filter(|s| is_strong_script_signal(s))
        .count();
    let confidence = compute_confidence(scriptable, strong_signals, occurrences, min_occurrences);
    RoutineSuggestion {
        suggested_script: scriptable.then(|| format!("scripts/{slug}.sh")),
        test_hint: scriptable.then(|| format!("tests/{slug}.sh with fixture files")),
        kind: if scriptable {
            RoutineKind::ScriptSkill
        } else {
            reasons.push(
                "kept as a normal skill suggestion because the steps are not all safely scriptable"
                    .to_string(),
            );
            RoutineKind::Skill
        },
        confidence,
        reasons,
        sequence,
        occurrences,
    }
}

/// 確信度を算出する (機械式3段階)。
///
/// - high: 必須条件すべて充足 かつ 強い判定 signal ≥2 かつ 出現回数 ≥ 2×閾値
/// - medium: 必須条件すべて充足 かつ 強い判定 signal ≥1
/// - low: それ以外 (script-skill にならない、または弱い)
fn compute_confidence(
    scriptable: bool,
    strong_signals: usize,
    occurrences: usize,
    min_occurrences: u32,
) -> Confidence {
    if !scriptable {
        return Confidence::Low;
    }
    if strong_signals >= 2 && occurrences >= (min_occurrences as usize * 2) {
        Confidence::High
    } else if strong_signals >= 1 {
        Confidence::Medium
    } else {
        Confidence::Low
    }
}

/// script-skill の必須条件を全て検査する。機械検出できる除外条件もここで弾く。
///
/// 除外条件 (機械検出・ハード除外):
/// - 破壊的操作 (Bash:rm 等): is_destructive_step
/// - 外部サービス/ネットワーク/認証系 (Bash:curl / Bash:gh / Bash:ssh 等): is_external_step
/// - 一度きり (出現回数が閾値未満): 呼び出し元で既に閾値フィルタ済み
/// - 自由編集操作を含む (Edit / Write 相当): is_deterministic_step が弾く
///
/// 意味判断系の除外 (設計判断/レビュー判断/人間承認/文章の意味判断/自由なコード編集の一部)
/// は、反復する機械コマンド列として検知されない時点で自然に script-skill 対象外。
/// usage の安全分類からは意味的検出ができないため明示フラグは設けない。
fn is_script_skill_candidate(sequence: &[String], reasons: &mut Vec<String>) -> bool {
    // 破壊的操作を含む列は降格。
    if sequence.iter().any(|step| is_destructive_step(step)) {
        return false;
    }
    // 外部サービス/ネットワーク/認証系を含む列は降格 (秘密値・認証情報の除外を兼ねる)。
    if sequence.iter().any(|step| is_external_step(step)) {
        return false;
    }
    // リポジトリ外を操作するステップを含む列は降格。
    if !sequence.iter().all(|step| is_repo_local_step(step)) {
        return false;
    }
    // 自由編集操作を含む列は決定的でないとみなし降格。
    if !sequence.iter().all(|step| is_deterministic_step(step)) {
        return false;
    }
    // 終了コードで失敗を表せるステップが無い列は script 化できない。
    if !sequence.iter().any(|step| has_exit_code_signal(step)) {
        return false;
    }
    // 強い判定条件が 1 つ以上なければ script-skill にしない。
    if !sequence.iter().any(|step| is_strong_script_signal(step)) {
        return false;
    }
    reasons.push("steps are repo-local and deterministic".to_string());
    reasons.push("the sequence includes a command whose exit code can express failure".to_string());
    reasons.push("the core steps match the safe script vocabulary".to_string());
    true
}

/// リポジトリ内で閉じるステップか (必須条件4)。
fn is_repo_local_step(step: &str) -> bool {
    matches!(
        step,
        "Read"
            | "Edit"
            | "Write"
            | "Bash:rg"
            | "Bash:sed"
            | "Bash:awk"
            | "Bash:jq"
            | "Bash:yq"
            | "Bash:cargo-test"
            | "Bash:npm-test"
            | "Bash:pytest"
            | "Bash:git-diff"
    )
}

/// 入力と出力がだいたい決まるステップか (必須条件2)。
///
/// 自由編集操作 (Edit / Write 相当) を含む列は「AIが自由にコードを変える」ため
/// 決定的でないとみなす。Edit/Write はリポジトリ内操作だが出力が自由なため除外。
/// これにより除外条件「AIの自由なコード編集が必要な作業」も同時に実装される。
///
/// is_repo_local_step とは独立した検査であることに注意:
/// - is_repo_local_step: リポジトリ外アクセスを弾く (外部性の検査)
/// - is_deterministic_step: 出力の自由度を弾く (決定論性の検査)
fn is_deterministic_step(step: &str) -> bool {
    // Edit / Write は自由編集 = 非決定的。
    !matches!(step, "Edit" | "Write")
}

/// 破壊的操作か (機械検出除外条件)。
fn is_destructive_step(step: &str) -> bool {
    matches!(step, "Bash:rm")
}

/// 外部サービス/ネットワーク/認証系のステップか (機械検出除外条件)。
///
/// これらを含む列は script-skill から降格する。秘密値・認証情報を扱う可能性 (必須条件5の裏)
/// をネットワーク/認証カテゴリの明示除外として実装している。
fn is_external_step(step: &str) -> bool {
    matches!(
        step,
        "Bash:curl" | "Bash:wget" | "Bash:gh" | "Bash:ssh" | "Bash:scp" | "Bash:rsync"
    )
}

fn has_exit_code_signal(step: &str) -> bool {
    matches!(
        step,
        "Bash:rg"
            | "Bash:sed"
            | "Bash:awk"
            | "Bash:jq"
            | "Bash:yq"
            | "Bash:cargo-test"
            | "Bash:npm-test"
            | "Bash:pytest"
            | "Bash:git-diff"
    )
}

fn is_strong_script_signal(step: &str) -> bool {
    matches!(
        step,
        "Bash:rg"
            | "Bash:sed"
            | "Bash:awk"
            | "Bash:jq"
            | "Bash:yq"
            | "Bash:cargo-test"
            | "Bash:npm-test"
            | "Bash:pytest"
            | "Bash:git-diff"
    )
}

/// 手順として意味があるか = 異なる name を 2 種以上含むか。
///
/// 同一 name の連続 (シェル操作の連打など) は技にできる「手順」でないので提案しない。
fn has_distinct_steps(sequence: &[String]) -> bool {
    sequence
        .iter()
        .collect::<std::collections::BTreeSet<_>>()
        .len()
        >= 2
}

/// `needle` が `haystack` の連続部分列か (同一は真)。
fn is_subsequence(needle: &[String], haystack: &[String]) -> bool {
    if needle.len() > haystack.len() {
        return false;
    }
    haystack.windows(needle.len()).any(|w| w == needle)
}

/// この手順が既にスキル化されているか (最小の名前照合)。
///
/// 列を slug 化し、いずれかのスキルの id / name の slug がそれを含めばスキル化済みとみなす。
/// 精緻な対応づけは持たない (`docs/decisions/...` の最小方針)。人間がこの手順用スキルを作り
/// 列名にちなんだ名前を付ければ再提案が止まる、という緩い結びつけ。
fn is_skilled(sequence: &[String], skills: &[Skill]) -> bool {
    let key = slugify_seq(sequence);
    skills
        .iter()
        .any(|s| normalize(&s.id).contains(&key) || normalize(&s.name).contains(&key))
}

/// name の列を `-` 連結の slug へ。各 name の記号は `-` に倒す。
fn slugify_seq(sequence: &[String]) -> String {
    sequence
        .iter()
        .map(|n| normalize(n))
        .collect::<Vec<_>>()
        .join("-")
}

/// 識別子を小文字化し、英数以外を `-` に倒して畳む。
fn normalize(s: &str) -> String {
    let mut out = String::new();
    let mut prev_dash = false;
    for c in s.trim().to_lowercase().chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c);
            prev_dash = false;
        } else if !prev_dash {
            out.push('-');
            prev_dash = true;
        }
    }
    out.trim_matches('-').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn tempdir() -> PathBuf {
        use std::sync::atomic::{AtomicU32, Ordering};
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let pid = std::process::id();
        let dir = std::env::temp_dir().join(format!("owox-routine-test-{pid}-{n}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn cfg() -> RoutineConfig {
        RoutineConfig {
            min_occurrences: 3,
            max_len: 4,
        }
    }

    fn seed(dir: &Path, pairs: &[&str]) {
        for name in pairs {
            usage::record(dir, "20260616", name);
        }
    }

    #[test]
    fn frequent_pair_is_suggested() {
        let dir = tempdir();
        // a,b を 3 回繰り返す (間に c を挟み隣接 a,b が 3 回)。
        seed(&dir, &["a", "b", "c", "a", "b", "c", "a", "b"]);
        let s = run_routine_suggestions(&dir, &cfg(), &[]);
        assert!(
            s.iter()
                .any(|r| r.sequence == vec!["a".to_string(), "b".to_string()] && r.occurrences >= 3)
        );
    }

    #[test]
    fn below_threshold_not_suggested() {
        let dir = tempdir();
        seed(&dir, &["a", "b", "a", "b"]); // a,b は 2 回 (閾値 3 未満)
        assert!(run_routine_suggestions(&dir, &cfg(), &[]).is_empty());
    }

    #[test]
    fn longer_sequence_subsumes_subsequence() {
        let dir = tempdir();
        // a,b,c を 3 回。a,b も 3 回現れるが長い列に含まれるので落ちる。
        seed(&dir, &["a", "b", "c", "a", "b", "c", "a", "b", "c"]);
        let s = run_routine_suggestions(&dir, &cfg(), &[]);
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].sequence, vec!["a", "b", "c"]);
        assert_eq!(s[0].kind, RoutineKind::Skill);
    }

    #[test]
    fn skilled_routine_filtered() {
        use crate::skill::Skill;
        let dir = tempdir();
        seed(&dir, &["build", "lint", "build", "lint", "build", "lint"]);
        let skill = Skill {
            id: "build-lint".to_string(),
            name: "build then lint".to_string(),
            description: "d".to_string(),
            skill_md: String::new(),
            implicit: false,
            promoted: false,
            human_gate: false,
            tests: Vec::new(),
            scripts: Vec::new(),
        };
        // build-lint スキルがあるので build,lint は再提案しない。
        let s = run_routine_suggestions(&dir, &cfg(), &[skill]);
        assert!(s.iter().all(|r| r.sequence != vec!["build", "lint"]));
    }

    #[test]
    fn repeated_single_name_is_not_a_routine() {
        let dir = tempdir();
        // 同じ name の連打は手順でない (distinct < 2)。提案しない。
        seed(&dir, &["Bash", "Bash", "Bash", "Bash", "Bash", "Bash"]);
        assert!(run_routine_suggestions(&dir, &cfg(), &[]).is_empty());
    }

    #[test]
    fn distinct_steps_within_noise_still_surface() {
        let dir = tempdir();
        // Bash 連打に混じる edit→verify の 2 種手順は拾う。
        seed(
            &dir,
            &[
                "Bash", "Bash", "edit", "verify", "Bash", "edit", "verify", "Bash", "edit",
                "verify", "edit", "verify", "edit", "verify",
            ],
        );
        let s = run_routine_suggestions(&dir, &cfg(), &[]);
        // edit→verify を含む手順が出る (長い形へ subsume されることはある)。
        assert!(s.iter().any(|r| {
            r.sequence.contains(&"edit".to_string()) && r.sequence.contains(&"verify".to_string())
        }));
        // どの提案も distinct >= 2 (Bash 単独連打は出ない)。
        assert!(!s.is_empty());
        assert!(s.iter().all(|r| has_distinct_steps(&r.sequence)));
    }

    #[test]
    fn safe_shell_vocab_promotes_to_script_skill() {
        let dir = tempdir();
        seed(
            &dir,
            &[
                "Read", "Bash:rg", "Bash:sed", "Read", "Bash:rg", "Bash:sed", "Read", "Bash:rg",
                "Bash:sed",
            ],
        );
        let s = run_routine_suggestions(&dir, &cfg(), &[]);
        assert!(s.iter().any(|r| {
            r.kind == RoutineKind::ScriptSkill
                && r.suggested_script.as_deref() == Some("scripts/read-bash-rg-bash-sed.sh")
                && !r.reasons.is_empty()
        }));
    }

    #[test]
    fn empty_usage_yields_nothing() {
        let dir = tempdir();
        assert!(run_routine_suggestions(&dir, &cfg(), &[]).is_empty());
    }

    // --- 追加テスト: 確定方針の実態検証 ---

    /// is_deterministic_step は Edit/Write を弾き、Read/Bash:rg は通す。
    /// is_repo_local_step は Edit/Write を通す = 別の独立した検査。
    #[test]
    fn deterministic_step_rejects_free_edit_independent_from_repo_local() {
        // Edit/Write は非決定的 (自由編集)。
        assert!(!is_deterministic_step("Edit"));
        assert!(!is_deterministic_step("Write"));
        // Read/Bash 系は決定的。
        assert!(is_deterministic_step("Read"));
        assert!(is_deterministic_step("Bash:rg"));
        assert!(is_deterministic_step("Bash:cargo-test"));
        // is_repo_local_step は Edit/Write を許す (リポジトリ内操作)。
        assert!(is_repo_local_step("Edit"));
        assert!(is_repo_local_step("Write"));
        // 独立性: 同じ入力で結果が異なる = 別の検査。
        assert_ne!(is_deterministic_step("Edit"), is_repo_local_step("Edit"));
    }

    /// Edit を含む列は script-skill にならない (自由編集 = 非決定的)。
    #[test]
    fn sequence_with_free_edit_stays_skill_not_script_skill() {
        let dir = tempdir();
        // Read → Edit → Bash:rg の列。Edit を含むため script-skill 不可。
        seed(
            &dir,
            &[
                "Read", "Edit", "Bash:rg", "Read", "Edit", "Bash:rg", "Read", "Edit", "Bash:rg",
            ],
        );
        let s = run_routine_suggestions(&dir, &cfg(), &[]);
        // Edit を含む列はすべて Skill 種別のまま。
        assert!(
            s.iter()
                .filter(|r| r.sequence.contains(&"Edit".to_string()))
                .all(|r| r.kind == RoutineKind::Skill)
        );
    }

    /// 破壊的操作 (Bash:rm) を含む列は script-skill から降格する。
    #[test]
    fn destructive_step_degrades_to_skill() {
        let dir = tempdir();
        seed(
            &dir,
            &[
                "Bash:rg", "Bash:rm", "Bash:rg", "Bash:rm", "Bash:rg", "Bash:rm",
            ],
        );
        let s = run_routine_suggestions(&dir, &cfg(), &[]);
        assert!(
            s.iter()
                .filter(|r| r.sequence.contains(&"Bash:rm".to_string()))
                .all(|r| r.kind == RoutineKind::Skill)
        );
    }

    /// 外部サービス操作 (Bash:curl) を含む列は script-skill から降格する。
    #[test]
    fn external_service_step_degrades_to_skill() {
        let dir = tempdir();
        seed(
            &dir,
            &[
                "Bash:rg",
                "Bash:curl",
                "Bash:rg",
                "Bash:curl",
                "Bash:rg",
                "Bash:curl",
            ],
        );
        let s = run_routine_suggestions(&dir, &cfg(), &[]);
        assert!(
            s.iter()
                .filter(|r| r.sequence.contains(&"Bash:curl".to_string()))
                .all(|r| r.kind == RoutineKind::Skill)
        );
    }

    /// ネットワーク系 (Bash:gh) を含む列は script-skill から降格する。
    #[test]
    fn network_step_degrades_to_skill() {
        let dir = tempdir();
        seed(
            &dir,
            &[
                "Bash:rg", "Bash:gh", "Bash:rg", "Bash:gh", "Bash:rg", "Bash:gh",
            ],
        );
        let s = run_routine_suggestions(&dir, &cfg(), &[]);
        assert!(
            s.iter()
                .filter(|r| r.sequence.contains(&"Bash:gh".to_string()))
                .all(|r| r.kind == RoutineKind::Skill)
        );
    }

    /// confidence は high/medium/low を条件に応じて切り替える。
    #[test]
    fn confidence_levels_computed_correctly() {
        // strong_signals=2 かつ occurrences >= 6 (= 2×3) → high。
        assert_eq!(compute_confidence(true, 2, 6, 3), Confidence::High);
        // strong_signals=2 だが occurrences < 6 → medium。
        assert_eq!(compute_confidence(true, 2, 5, 3), Confidence::Medium);
        // strong_signals=1 かつ occurrences < 6 → medium。
        assert_eq!(compute_confidence(true, 1, 3, 3), Confidence::Medium);
        // scriptable=false → low。
        assert_eq!(compute_confidence(false, 2, 6, 3), Confidence::Low);
        // strong_signals=0 → low。
        assert_eq!(compute_confidence(true, 0, 6, 3), Confidence::Low);
    }

    /// script-skill 候補は confidence が medium 以上になる (strong signal ≥1 必須)。
    #[test]
    fn script_skill_has_medium_or_high_confidence() {
        let dir = tempdir();
        seed(
            &dir,
            &[
                "Read", "Bash:rg", "Bash:sed", "Read", "Bash:rg", "Bash:sed", "Read", "Bash:rg",
                "Bash:sed",
            ],
        );
        let s = run_routine_suggestions(&dir, &cfg(), &[]);
        let script_skills: Vec<_> = s
            .iter()
            .filter(|r| r.kind == RoutineKind::ScriptSkill)
            .collect();
        assert!(!script_skills.is_empty());
        assert!(
            script_skills
                .iter()
                .all(|r| r.confidence == Confidence::Medium || r.confidence == Confidence::High)
        );
    }

    /// RoutineConfig のデフォルト min_occurrences は 3 (要件初期値)。テスト専用 cfg でなく本番デフォルト。
    #[test]
    fn default_min_occurrences_is_3() {
        let cfg = RoutineConfig::default();
        assert_eq!(cfg.min_occurrences, 3, "要件初期値 3 に一致する必要がある");
    }
}
