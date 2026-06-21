//! quality.toml: 品質バーの適応度関数 (`docs/decisions/20260614-Phase6-quality適応度関数.md`)。
//!
//! owox は言語非依存で構文解析を持たない。ファイルを見るだけで分かる2種だけ直接検証する:
//! ファイル行数予算と禁止パターン (層境界/依存方向)。循環・複雑度は config の検査コマンドへ委譲する。
//!
//! ファイル列挙は呼び出し側 (mcp) が git ls-files 等で与える。core は git/走査を持たず決定論
//! (today / known_checks と同じく外から受ける)。glob は新規依存を足さず regex へ変換して照合する。

use std::path::Path;

use regex::Regex;
use serde::Deserialize;

use crate::model::{ForbiddenTerm, VerifyCheck};

/// 品質バー。quality.toml の型付き表現。無ければ無効 (opt-in)。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Quality {
    /// ファイル行数予算。
    pub budgets: Vec<SizeBudget>,
    /// 層境界/依存方向。禁止パターンで表す。
    pub boundaries: Vec<Boundary>,
    /// 腐敗検知の閾値 (`[decay]`)。無くても既定値で検知する (`decay.rs`)。
    pub decay: DecayConfig,
    /// 頻出手順検知の閾値 (`[routine]`)。無くても既定値で検知する (`routine.rs`)。
    pub routine: RoutineConfig,
    /// 層別自律度 (`[[layers]]`)。AI が人間承認なしで変えてよい度合いを層ごとに変える。
    /// architecture=layered の時だけ効く層機構 (`docs/decisions/20260618-Phase9-性質軸適応機構.md`)。
    pub layers: Vec<Layer>,
}

/// 層の自律度。核ほど慎重。ゲート合成のはしごへ写像する (`gate.rs`)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Autonomy {
    /// 核/内側。AI が人間承認なしで変えてはいけない。
    Guarded,
    /// 中間。助言+記録。phase が厳しさを変調する。
    Supervised,
    /// 外側。安全ゲートのみ。AI 自由。
    Free,
}

impl Autonomy {
    pub fn parse(value: &str) -> Result<Self, String> {
        match value.trim() {
            "guarded" => Ok(Self::Guarded),
            "supervised" => Ok(Self::Supervised),
            "free" => Ok(Self::Free),
            other => Err(format!(
                "layer の autonomy は guarded / supervised / free のみ: {other}"
            )),
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Guarded => "guarded",
            Self::Supervised => "supervised",
            Self::Free => "free",
        }
    }
}

/// 層 1 件。paths (glob) 配下のファイルが autonomy を持つ。
/// contract_surface は guarded 層のうち「契約面」とみなすパス (編集を操作前ゲートする)。
/// name は層別充足報告で要件/タスクの layer タグと突き合わせる層名 (任意)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layer {
    pub name: Option<String>,
    pub paths: Vec<String>,
    pub autonomy: Autonomy,
    pub contract_surface: Vec<String>,
}

/// 頻出手順検知の閾値。quality.toml の `[routine]` 節。DecayConfig と同流儀で既定で検知する
/// (`docs/decisions/20260616-Phase8-パターンからスキル育成.md`)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoutineConfig {
    /// 隣接 name 列がこの回数以上現れたら提案する。
    pub min_occurrences: u32,
    /// 数える隣接 name 列の最大長 (2..=max_len)。
    pub max_len: usize,
}

impl Default for RoutineConfig {
    fn default() -> Self {
        // 既定は緩め。狼少年化を避け、対話検証で詰める。
        RoutineConfig {
            min_occurrences: 5,
            max_len: 4,
        }
    }
}

/// 腐敗検知の閾値。quality.toml の `[decay]` 節。budgets/boundaries と違い既定で検知する
/// (腐敗防止は中核必須・後付けにしない。`docs/decisions/20260611-タスク管理.md`)。
#[derive(Debug, Clone, PartialEq)]
pub struct DecayConfig {
    /// 未完タスクをこの日数以上放置で警告。
    pub stale_task_days: u32,
    /// open の来歴をこの日数以上放置で警告。
    pub open_decision_days: u32,
    /// adopted の来歴をこの日数以上で見直し合図。
    pub review_decision_days: u32,
    /// current の調査知識をこの日数以上で鮮度警告 (`docs/decisions/20260616-Phase8-調査知識層.md`)。
    pub knowledge_stale_days: u32,
    /// ブランチ作業記憶をこの日数以上放置で鮮度警告 (`docs/decisions/20260618-Phase9-ブランチ作業記憶層.md`)。
    pub branch_memory_stale_days: u32,
    /// practice 対の字 n-gram Jaccard 類似度がこれ以上で冗長と報告
    /// (`docs/decisions/20260617-practices冗長性の機械シグナル.md`)。
    pub practice_similarity: f64,
    /// 重複ファイル検出の最小サイズ。これ未満は空・極小として対象外
    /// (`docs/decisions/20260614-Phase7-コードrepo腐敗検知.md`)。
    pub min_duplicate_bytes: usize,
    /// 委譲検出 (`[[decay.checks]]`)。owox が走らせ非ゼロ終了を decay として報告する advisory な検査。
    /// 完了を判定する `[[verify.checks]]` と区別する。
    pub checks: Vec<VerifyCheck>,
}

impl Default for DecayConfig {
    fn default() -> Self {
        // 既定は緩め。狼少年化を避け、対話検証で詰める (`docs/decisions/20260614-Phase7-腐敗検知の中核.md`)。
        DecayConfig {
            stale_task_days: 14,
            open_decision_days: 7,
            review_decision_days: 90,
            // 来歴の見直しと揃える (調査も同周期で見直す)。
            knowledge_stale_days: 90,
            // ブランチは短命なので調査より短く。実機で調整。
            branch_memory_stale_days: 30,
            // 緩めに始め実機で調整 (狼少年化を避ける)。
            practice_similarity: 0.5,
            min_duplicate_bytes: 64,
            checks: Vec::new(),
        }
    }
}

/// ファイル行数予算 1 件。paths (glob) に当たるファイルは max_lines 行以下。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SizeBudget {
    pub paths: Vec<String>,
    pub max_lines: usize,
}

/// 層境界 1 件。paths (glob) 配下のファイルに forbid (正規表現) が現れたら違反。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Boundary {
    pub paths: Vec<String>,
    pub forbid: Vec<String>,
    pub reason: Option<String>,
}

/// 品質違反 1 件。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QualityViolation {
    /// budget / boundary。
    pub kind: &'static str,
    /// 違反したファイル (work_dir からの相対)。
    pub path: String,
    /// 人間向けの説明。
    pub detail: String,
}

impl QualityViolation {
    /// 1 行サマリ (commit ゲートのメッセージ・封筒に使う)。
    pub fn summary(&self) -> String {
        format!("{} [{}]: {}", self.path, self.kind, self.detail)
    }
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct QualityRaw {
    #[serde(default)]
    budgets: Vec<BudgetRaw>,
    #[serde(default)]
    boundaries: Vec<BoundaryRaw>,
    #[serde(default)]
    decay: DecayRaw,
    #[serde(default)]
    routine: RoutineRaw,
    #[serde(default)]
    layers: Vec<LayerRaw>,
}

/// `[[layers]]` の生表現。autonomy は文字列で受け Autonomy::parse で検証する。
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LayerRaw {
    #[serde(default)]
    name: Option<String>,
    paths: Vec<String>,
    autonomy: String,
    #[serde(default)]
    contract_surface: Vec<String>,
}

/// `[routine]` の生表現。各値は省略可で、省略時は RoutineConfig の既定値。
#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct RoutineRaw {
    min_occurrences: Option<u32>,
    max_len: Option<usize>,
}

/// `[decay]` の生表現。各値は省略可で、省略時は DecayConfig の既定値。
#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct DecayRaw {
    stale_task_days: Option<u32>,
    open_decision_days: Option<u32>,
    review_decision_days: Option<u32>,
    knowledge_stale_days: Option<u32>,
    branch_memory_stale_days: Option<u32>,
    practice_similarity: Option<f64>,
    min_duplicate_bytes: Option<usize>,
    #[serde(default)]
    checks: Vec<DecayCheckRaw>,
}

/// `[[decay.checks]]` の生表現。model の VerifyCheckRaw と同じ流儀で解釈する。
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DecayCheckRaw {
    name: String,
    command: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BudgetRaw {
    paths: Vec<String>,
    max_lines: usize,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BoundaryRaw {
    paths: Vec<String>,
    forbid: Vec<String>,
    #[serde(default)]
    reason: Option<String>,
}

impl Quality {
    /// quality.toml を読み型へ検証する。未知キーと forbid の不正な正規表現を弾く。
    pub fn from_toml(text: &str) -> Result<Quality, String> {
        let raw: QualityRaw = toml::from_str(text).map_err(|e| e.to_string())?;

        // forbid の正規表現を読込時に検証する (irreversible の detect: と同じく誤記を早期に弾く)。
        for b in &raw.boundaries {
            for f in &b.forbid {
                Regex::new(f)
                    .map_err(|e| format!("boundary の forbid 正規表現が不正: {f}: {e}"))?;
            }
        }

        let d = DecayConfig::default();
        Ok(Quality {
            budgets: raw
                .budgets
                .into_iter()
                .map(|b| SizeBudget {
                    paths: b.paths,
                    max_lines: b.max_lines,
                })
                .collect(),
            boundaries: raw
                .boundaries
                .into_iter()
                .map(|b| Boundary {
                    paths: b.paths,
                    forbid: b.forbid,
                    reason: b.reason,
                })
                .collect(),
            decay: DecayConfig {
                stale_task_days: raw.decay.stale_task_days.unwrap_or(d.stale_task_days),
                open_decision_days: raw.decay.open_decision_days.unwrap_or(d.open_decision_days),
                review_decision_days: raw
                    .decay
                    .review_decision_days
                    .unwrap_or(d.review_decision_days),
                knowledge_stale_days: raw
                    .decay
                    .knowledge_stale_days
                    .unwrap_or(d.knowledge_stale_days),
                branch_memory_stale_days: raw
                    .decay
                    .branch_memory_stale_days
                    .unwrap_or(d.branch_memory_stale_days),
                practice_similarity: raw
                    .decay
                    .practice_similarity
                    .unwrap_or(d.practice_similarity),
                min_duplicate_bytes: raw
                    .decay
                    .min_duplicate_bytes
                    .unwrap_or(d.min_duplicate_bytes),
                checks: raw
                    .decay
                    .checks
                    .into_iter()
                    .map(|c| VerifyCheck {
                        name: c.name,
                        command: c.command,
                    })
                    .collect(),
            },
            routine: {
                let r = RoutineConfig::default();
                RoutineConfig {
                    min_occurrences: raw.routine.min_occurrences.unwrap_or(r.min_occurrences),
                    max_len: raw.routine.max_len.unwrap_or(r.max_len),
                }
            },
            layers: raw
                .layers
                .into_iter()
                .map(|l| {
                    Ok(Layer {
                        name: l.name.filter(|s| !s.trim().is_empty()),
                        paths: l.paths,
                        autonomy: Autonomy::parse(&l.autonomy)?,
                        contract_surface: l.contract_surface,
                    })
                })
                .collect::<Result<Vec<_>, String>>()?,
        })
    }

    fn is_empty(&self) -> bool {
        self.budgets.is_empty() && self.boundaries.is_empty()
    }

    /// path が属する層の自律度。宣言順で最初に当たった層を採る (具体的な層を先に書く)。
    /// どの層にも当たらなければ Free (外側既定)。
    pub fn layer_autonomy(&self, path: &str) -> Autonomy {
        for l in &self.layers {
            if compile_globs(&l.paths).iter().any(|re| re.is_match(path)) {
                return l.autonomy;
            }
        }
        Autonomy::Free
    }

    /// path が guarded 層の契約面か (操作前ゲートの対象)。
    /// path が属する最初の層が guarded で、その contract_surface に当たる時だけ true。
    pub fn is_contract_surface(&self, path: &str) -> bool {
        for l in &self.layers {
            if compile_globs(&l.paths).iter().any(|re| re.is_match(path)) {
                return l.autonomy == Autonomy::Guarded
                    && compile_globs(&l.contract_surface)
                        .iter()
                        .any(|re| re.is_match(path));
            }
        }
        false
    }

    /// quality.toml で宣言された層名の一覧 (name を持つ層だけ)。
    /// 要件/タスクの layer タグをこの集合へ照合する。空なら検証しない (任意宣言)。
    pub fn layer_names(&self) -> Vec<String> {
        self.layers.iter().filter_map(|l| l.name.clone()).collect()
    }
}

/// layer タグが quality.toml で宣言された層名か照合する。
///
/// 層別充足報告とゲート層の真実を一致させる (`docs/decisions/20260618-Phase9-性質軸適応機構.md`)。
/// known_layers が空 = 層名が未宣言のため照合しない (任意宣言・requirement の check_known と同型)。
pub(crate) fn check_known_layer(layer: &str, known_layers: &[String]) -> Result<(), String> {
    let l = layer.trim();
    if l.is_empty() || known_layers.is_empty() || known_layers.iter().any(|k| k == l) {
        return Ok(());
    }
    Err(format!(
        "Unknown layer: {l}. Use a layer declared in quality.toml (declared: {}) or add it to [[layers]] first.",
        known_layers.join(", ")
    ))
}

/// 品質バーを検証する。`files` は work_dir からの相対パス (mcp が列挙して渡す)。
///
/// glob に当たるファイルだけ読み、行数予算と禁止パターンを照合する。読めないファイルは飛ばす。
pub fn run_quality(quality: &Quality, work_dir: &Path, files: &[String]) -> Vec<QualityViolation> {
    if quality.is_empty() {
        return Vec::new();
    }

    // glob と forbid 正規表現を事前にコンパイルする。
    let budgets: Vec<(Vec<Regex>, usize)> = quality
        .budgets
        .iter()
        .map(|b| (compile_globs(&b.paths), b.max_lines))
        .collect();
    let boundaries: Vec<CompiledBoundary> = quality
        .boundaries
        .iter()
        .map(|b| {
            let globs = compile_globs(&b.paths);
            let pats: Vec<(String, Regex)> = b
                .forbid
                .iter()
                .filter_map(|f| Regex::new(f).ok().map(|re| (f.clone(), re)))
                .collect();
            (globs, pats, b.reason.clone())
        })
        .collect();

    let mut violations = Vec::new();
    for file in files {
        let budget_maxes: Vec<usize> = budgets
            .iter()
            .filter(|(globs, _)| globs.iter().any(|g| g.is_match(file)))
            .map(|(_, max)| *max)
            .collect();
        let boundary_hits: Vec<usize> = boundaries
            .iter()
            .enumerate()
            .filter(|(_, (globs, _, _))| globs.iter().any(|g| g.is_match(file)))
            .map(|(i, _)| i)
            .collect();

        if budget_maxes.is_empty() && boundary_hits.is_empty() {
            continue;
        }

        let Ok(content) = std::fs::read_to_string(work_dir.join(file)) else {
            continue;
        };

        let lines = content.lines().count();
        for max in budget_maxes {
            if lines > max {
                violations.push(QualityViolation {
                    kind: "budget",
                    path: file.clone(),
                    detail: format!("{lines} lines exceed the {max}-line budget"),
                });
            }
        }
        for bidx in boundary_hits {
            let (_, pats, reason) = &boundaries[bidx];
            for (pat, re) in pats {
                if re.is_match(&content) {
                    let why = reason
                        .as_deref()
                        .map(|r| format!(" ({r})"))
                        .unwrap_or_default();
                    violations.push(QualityViolation {
                        kind: "boundary",
                        path: file.clone(),
                        detail: format!("matches forbidden pattern /{pat}/{why}"),
                    });
                }
            }
        }
    }
    violations
}

/// ブランドの禁止語を検証する。`files` は work_dir からの相対パス (mcp が列挙して渡す)。
///
/// 禁止語 (glossary.md の予約見出し Forbidden) を追跡テキストファイルへ照合し、
/// 当たれば QualityViolation { kind:"brand" } を作る。報告チャネルは quality 違反と同じで、
/// commit ゲートの phase 適応もそのまま乗る (`docs/decisions/20260614-Phase7-測定可視化とブランド検証.md`)。
/// 禁止語は語彙の正本、quality.toml の boundaries は正規表現の汎用枠と役割を分ける。
pub fn run_brand(
    forbidden: &[ForbiddenTerm],
    work_dir: &Path,
    files: &[String],
) -> Vec<QualityViolation> {
    if forbidden.is_empty() {
        return Vec::new();
    }

    // 禁止語の正規表現を事前にコンパイルする (読込時に検証済みだが失敗は無視)。
    let pats: Vec<(&ForbiddenTerm, Regex)> = forbidden
        .iter()
        .filter_map(|f| Regex::new(&f.pattern).ok().map(|re| (f, re)))
        .collect();

    let mut violations = Vec::new();
    for file in files {
        // テキストとして読めるファイルのみ照合する (バイナリは飛ばす)。
        let Ok(content) = std::fs::read_to_string(work_dir.join(file)) else {
            continue;
        };
        for (term, re) in &pats {
            if re.is_match(&content) {
                let why = if term.reason.is_empty() {
                    String::new()
                } else {
                    format!(" ({})", term.reason)
                };
                violations.push(QualityViolation {
                    kind: "brand",
                    path: file.clone(),
                    detail: format!("contains forbidden term /{}/{why}", term.pattern),
                });
            }
        }
    }
    violations
}

/// コンパイル済み境界 = (paths の glob, (元の正規表現文字列, コンパイル済み) の並び, reason)。
type CompiledBoundary = (Vec<Regex>, Vec<(String, Regex)>, Option<String>);

fn compile_globs(globs: &[String]) -> Vec<Regex> {
    globs.iter().map(|g| glob_to_regex(g)).collect()
}

/// glob をアンカー付きの正規表現へ変換する (新規依存を足さない)。
///
/// 対応: `*` = 区切り内の任意、`**` = 任意の深さ、`/**/` = 0 個以上のディレクトリ、`?` = 区切り内 1 文字。
/// それ以外の文字は正規表現メタ文字を退避して literal にする。
/// レビュー観点の適用トリガ (review.rs) でも再利用する。
pub(crate) fn glob_to_regex(glob: &str) -> Regex {
    let chars: Vec<char> = glob.chars().collect();
    let mut out = String::from("^");
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '*' if i + 1 < chars.len() && chars[i + 1] == '*' => {
                if i + 2 < chars.len() && chars[i + 2] == '/' {
                    // `/**/` は 0 個以上のディレクトリにマッチさせる。
                    out.push_str("(?:.*/)?");
                    i += 3;
                } else {
                    out.push_str(".*");
                    i += 2;
                }
            }
            '*' => {
                out.push_str("[^/]*");
                i += 1;
            }
            '?' => {
                out.push_str("[^/]");
                i += 1;
            }
            c => {
                if "\\.+()|[]{}^$".contains(c) {
                    out.push('\\');
                }
                out.push(c);
                i += 1;
            }
        }
    }
    out.push('$');
    Regex::new(&out).expect("glob から作った正規表現は妥当")
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
        let dir = std::env::temp_dir().join(format!("owox-quality-test-{pid}-{n}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write(dir: &Path, rel: &str, content: &str) {
        let path = dir.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }

    #[test]
    fn glob_matches_expected_paths() {
        let g = glob_to_regex("src/domain/**");
        assert!(g.is_match("src/domain/order.rs"));
        assert!(g.is_match("src/domain/sub/order.rs"));
        assert!(!g.is_match("src/infra/db.rs"));

        let g = glob_to_regex("src/**/*.rs");
        assert!(g.is_match("src/a.rs"));
        assert!(g.is_match("src/x/y/a.rs"));
        assert!(!g.is_match("src/a.txt"));

        let g = glob_to_regex("*.rs");
        assert!(g.is_match("main.rs"));
        assert!(!g.is_match("src/main.rs"));
    }

    #[test]
    fn budget_flags_files_over_limit() {
        let dir = tempdir();
        write(&dir, "src/big.rs", "a\nb\nc\nd\n"); // 4 行
        write(&dir, "src/small.rs", "a\n");
        let q = Quality {
            budgets: vec![SizeBudget {
                paths: vec!["src/**/*.rs".to_string()],
                max_lines: 3,
            }],
            boundaries: Vec::new(),
            ..Quality::default()
        };
        let files = vec!["src/big.rs".to_string(), "src/small.rs".to_string()];
        let v = run_quality(&q, &dir, &files);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].kind, "budget");
        assert_eq!(v[0].path, "src/big.rs");
    }

    #[test]
    fn boundary_flags_forbidden_pattern() {
        let dir = tempdir();
        write(&dir, "src/domain/order.rs", "use crate::infra::db;\n");
        write(&dir, "src/domain/clean.rs", "use crate::domain::x;\n");
        let q = Quality {
            budgets: Vec::new(),
            boundaries: vec![Boundary {
                paths: vec!["src/domain/**".to_string()],
                forbid: vec!["use .*infra".to_string()],
                reason: Some("domain must not depend on infra".to_string()),
            }],
            ..Quality::default()
        };
        let files = vec![
            "src/domain/order.rs".to_string(),
            "src/domain/clean.rs".to_string(),
        ];
        let v = run_quality(&q, &dir, &files);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].kind, "boundary");
        assert_eq!(v[0].path, "src/domain/order.rs");
        assert!(v[0].detail.contains("domain must not depend on infra"));
    }

    #[test]
    fn empty_quality_finds_nothing() {
        let dir = tempdir();
        write(&dir, "a.rs", "x\n");
        let v = run_quality(&Quality::default(), &dir, &["a.rs".to_string()]);
        assert!(v.is_empty());
    }

    #[test]
    fn invalid_forbid_regex_rejected_at_load() {
        let err =
            Quality::from_toml("[[boundaries]]\npaths=[\"src/**\"]\nforbid=[\"(unclosed\"]\n")
                .unwrap_err();
        assert!(err.contains("forbid"), "{err}");
    }

    #[test]
    fn from_toml_reads_budgets_and_boundaries() {
        let q = Quality::from_toml(
            "[[budgets]]\npaths=[\"src/**/*.rs\"]\nmax_lines=400\n\n[[boundaries]]\npaths=[\"src/domain/**\"]\nforbid=[\"infra\"]\nreason=\"layering\"\n",
        )
        .unwrap();
        assert_eq!(q.budgets.len(), 1);
        assert_eq!(q.budgets[0].max_lines, 400);
        assert_eq!(q.boundaries.len(), 1);
        assert_eq!(q.boundaries[0].reason.as_deref(), Some("layering"));
    }

    #[test]
    fn unknown_key_rejected() {
        let err =
            Quality::from_toml("[[budgets]]\npaths=[\"x\"]\nmax_lines=1\nbogus=2\n").unwrap_err();
        assert!(!err.is_empty());
    }

    #[test]
    fn brand_flags_forbidden_term() {
        let dir = tempdir();
        write(&dir, "docs/a.md", "これは foobar を含む文章\n");
        write(&dir, "docs/b.md", "正常な文章\n");
        let forbidden = vec![ForbiddenTerm {
            pattern: r"\bfoobar\b".to_string(),
            reason: "造語禁止".to_string(),
        }];
        let files = vec!["docs/a.md".to_string(), "docs/b.md".to_string()];
        let v = run_brand(&forbidden, &dir, &files);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].kind, "brand");
        assert_eq!(v[0].path, "docs/a.md");
        assert!(v[0].detail.contains("造語禁止"));
    }

    #[test]
    fn brand_empty_finds_nothing() {
        let dir = tempdir();
        write(&dir, "a.md", "foobar\n");
        let v = run_brand(&[], &dir, &["a.md".to_string()]);
        assert!(v.is_empty());
    }

    #[test]
    fn layer_name_parsed_and_collected() {
        let q = Quality::from_toml(
            "[[layers]]\nname=\"core\"\npaths=[\"src/core/**\"]\nautonomy=\"guarded\"\n\n[[layers]]\npaths=[\"src/app/**\"]\nautonomy=\"free\"\n",
        )
        .unwrap();
        assert_eq!(q.layers.len(), 2);
        assert_eq!(q.layers[0].name.as_deref(), Some("core"));
        assert_eq!(q.layers[1].name, None);
        // name を持つ層だけ集める。
        assert_eq!(q.layer_names(), vec!["core".to_string()]);
    }

    #[test]
    fn check_known_layer_behaviour() {
        let known = vec!["core".to_string(), "infra".to_string()];
        // 空 layer は常に許す。
        assert!(check_known_layer("", &known).is_ok());
        // 宣言済なら許す。
        assert!(check_known_layer("core", &known).is_ok());
        // 未知は弾き有効名を添える。
        let err = check_known_layer("ghost", &known).unwrap_err();
        assert!(err.contains("core, infra"));
        // 層名が未宣言 (空) なら任意 layer を許す。
        assert!(check_known_layer("anything", &[]).is_ok());
    }
}
