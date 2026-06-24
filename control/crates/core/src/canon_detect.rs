//! 既存コードからの canon 逆生成 (kickoff 拡張)。
//!
//! profile.detect が性質4軸を推定するのに続き、本モジュールは既存 repo から
//! rules (不可逆操作) と quality (層・層境界) の初期案を出す
//! (`docs/decisions/20260611-方向付け.md` の kickoff 拡張・要件 C群)。
//!
//! 出力は draft + 根拠のみ。確定しない: 人間が確認して canon へ書く (profile.detect と同じ
//! 人間ゲート思想)。シグナルは安いファイル名ベースで言語非依存。生成した正規表現は妥当性を
//! 検証してから返し、壊れた draft を出さない。

use crate::profile::DetectSignals;

/// 逆生成した層 1 件の draft。paths 配下を autonomy で守る案。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayerDraft {
    pub name: String,
    pub paths: Vec<String>,
    /// guarded / free。owox 層別自律度の語彙。
    pub autonomy: &'static str,
    pub evidence: String,
}

/// 逆生成した層境界 1 件の draft。paths 配下に forbid (正規表現) が出たら違反の案。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoundaryDraft {
    pub paths: Vec<String>,
    pub forbid: Vec<String>,
    pub reason: String,
    pub evidence: String,
}

/// 逆生成した不可逆操作 1 件の draft。detect はコマンド正規表現の案。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IrreversibleDraft {
    pub operation: String,
    pub reason: String,
    pub detect: String,
    pub evidence: String,
}

/// 逆生成した canon の初期案。全て draft で確定しない。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CanonDraft {
    pub layers: Vec<LayerDraft>,
    pub boundaries: Vec<BoundaryDraft>,
    pub irreversible: Vec<IrreversibleDraft>,
}

impl CanonDraft {
    /// 何も検出しなかったか。kickoff が「初期案なし」を伝える判定に使う。
    pub fn is_empty(&self) -> bool {
        self.layers.is_empty() && self.boundaries.is_empty() && self.irreversible.is_empty()
    }
}

/// 層ディレクトリ → (役割名, autonomy)。core/契約は guarded・端は free。
/// glob は `**/<dir>/**` で深さ非依存に拾う。
const CORE_LAYER_DIRS: &[&str] = &["domain", "entities", "usecase", "usecases", "ports", "core"];
const EDGE_LAYER_DIRS: &[&str] = &["infra", "infrastructure", "adapters", "adapter"];

/// path 群のいずれかが `/<dir>/` を含むか (小文字比較)。先頭の `<dir>/` も拾う。
fn dir_present(files: &[String], dir: &str) -> bool {
    let needle = format!("/{dir}/");
    let prefix = format!("{dir}/");
    files.iter().any(|f| {
        let lower = f.to_lowercase();
        lower.contains(&needle) || lower.starts_with(&prefix)
    })
}

/// path 群のいずれかが substr を含むか (小文字比較)。
fn any_contains(files: &[String], substr: &str) -> bool {
    files.iter().any(|f| f.to_lowercase().contains(substr))
}

/// path 群のいずれかが拡張子で終わるか (小文字比較)。
fn any_ends_with(files: &[String], ext: &str) -> bool {
    files.iter().any(|f| f.to_lowercase().ends_with(ext))
}

/// 既存コードから rules / quality の初期案を逆生成する。
///
/// - 層: core 系/端系ディレクトリの存在から `[[layers]]` 案を出す
/// - 層境界: core と端が両在する時、core が端へ依存しない方向境界を出す (依存方向の起点)
/// - 不可逆: migrations / terraform / kubernetes の痕跡から守るべきコマンド案を出す
///
/// 出力は draft。detect 正規表現は妥当性検証済 (壊れた案を出さない)。
pub fn detect_canon_draft(sig: &DetectSignals) -> CanonDraft {
    let mut draft = CanonDraft::default();

    // 層: 存在する core 系は guarded・端系は free で案にする。
    let mut present_core: Vec<&str> = Vec::new();
    let mut present_edge: Vec<&str> = Vec::new();
    for dir in CORE_LAYER_DIRS {
        if dir_present(sig.files, dir) {
            present_core.push(dir);
            draft.layers.push(LayerDraft {
                name: (*dir).to_string(),
                paths: vec![format!("**/{dir}/**")],
                autonomy: "guarded",
                evidence: format!("a {dir}/ directory is present (core / contract layer)"),
            });
        }
    }
    for dir in EDGE_LAYER_DIRS {
        if dir_present(sig.files, dir) {
            present_edge.push(dir);
            draft.layers.push(LayerDraft {
                name: (*dir).to_string(),
                paths: vec![format!("**/{dir}/**")],
                autonomy: "free",
                evidence: format!("a {dir}/ directory is present (edge / infrastructure layer)"),
            });
        }
    }

    // 層境界: core と端が両在するなら依存方向の起点を出す。
    // core 配下が端ディレクトリ名を参照したら違反 (依存方向: core は端へ依存しない)。
    // 正規表現は端ディレクトリ名の語境界一致 (言語非依存の起点・人間が言語の import 構文へ寄せる)。
    if !present_core.is_empty() && !present_edge.is_empty() {
        let core_paths: Vec<String> = present_core.iter().map(|d| format!("**/{d}/**")).collect();
        let forbid: Vec<String> = present_edge
            .iter()
            .map(|d| format!(r"\b{}\b", regex::escape(d)))
            .filter(|re| regex::Regex::new(re).is_ok())
            .collect();
        if !forbid.is_empty() {
            draft.boundaries.push(BoundaryDraft {
                paths: core_paths,
                forbid,
                reason: "dependency direction: the core layer must not depend on infrastructure"
                    .to_string(),
                evidence: format!(
                    "both core ({}) and edge ({}) layers are present",
                    present_core.join(", "),
                    present_edge.join(", ")
                ),
            });
        }
    }

    // 不可逆: 痕跡から守るべき破壊的コマンドの detect 案を出す。妥当な正規表現だけ残す。
    let mut push_irrev = |present: bool, op: &str, reason: &str, detect: &str, evidence: &str| {
        if present && regex::Regex::new(detect).is_ok() {
            draft.irreversible.push(IrreversibleDraft {
                operation: op.to_string(),
                reason: reason.to_string(),
                detect: detect.to_string(),
                evidence: evidence.to_string(),
            });
        }
    };

    push_irrev(
        any_contains(sig.files, "migration") || any_contains(sig.files, "/migrate/"),
        "Roll back or reset a database migration",
        "schema changes against a real database are hard to undo and can lose data",
        r"\bmigrat(e|ion)\b.*\b(down|reset|rollback|drop)\b",
        "migration files are present",
    );
    push_irrev(
        any_ends_with(sig.files, ".tf") || any_contains(sig.files, "terraform"),
        "Apply or destroy Terraform-managed infrastructure",
        "terraform apply / destroy mutates real cloud resources irreversibly",
        r"terraform\s+(apply|destroy)",
        "Terraform files are present",
    );
    push_irrev(
        any_contains(sig.files, "/k8s/")
            || any_contains(sig.files, "kubernetes")
            || any_contains(sig.files, "helm")
            || any_contains(sig.files, "/charts/"),
        "Delete or apply Kubernetes / Helm resources",
        "kubectl delete / helm uninstall removes live cluster state irreversibly",
        r"(kubectl\s+delete|helm\s+(uninstall|delete))",
        "Kubernetes / Helm manifests are present",
    );

    draft
}

/// quality.toml へ貼れる `[[layers]]` / `[[boundaries]]` 断片を描く。
/// 何も無ければ空文字 (kickoff が「層の初期案なし」を判断できる)。
pub fn render_quality_toml(draft: &CanonDraft) -> String {
    let mut out = String::new();
    for l in &draft.layers {
        let paths = l
            .paths
            .iter()
            .map(|p| format!("\"{p}\""))
            .collect::<Vec<_>>()
            .join(", ");
        out.push_str(&format!(
            "# {evidence}\n[[layers]]\nname = \"{name}\"\npaths = [{paths}]\nautonomy = \"{autonomy}\"\n\n",
            evidence = l.evidence,
            name = l.name,
            autonomy = l.autonomy,
        ));
    }
    for b in &draft.boundaries {
        let paths = b
            .paths
            .iter()
            .map(|p| format!("\"{p}\""))
            .collect::<Vec<_>>()
            .join(", ");
        let forbid = b
            .forbid
            .iter()
            .map(|p| format!("\"{}\"", p.replace('\\', "\\\\")))
            .collect::<Vec<_>>()
            .join(", ");
        out.push_str(&format!(
            "# {evidence}\n[[boundaries]]\npaths = [{paths}]\nforbid = [{forbid}]\nreason = \"{reason}\"\n\n",
            evidence = b.evidence,
            reason = b.reason,
        ));
    }
    out
}

/// rules.md の `Irreversible operations` 節へ貼れる箇条書きを描く。
/// 何も無ければ空文字。
pub fn render_rules_markdown(draft: &CanonDraft) -> String {
    let mut out = String::new();
    for i in &draft.irreversible {
        out.push_str(&format!(
            "- {op}: {reason}\n  detect: {detect}\n",
            op = i.operation,
            reason = i.reason,
            detect = i.detect,
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sig(files: &[&str]) -> Vec<String> {
        files.iter().map(|s| s.to_string()).collect()
    }

    fn detect(files: &[String]) -> CanonDraft {
        detect_canon_draft(&DetectSignals {
            files,
            has_quality_layers: false,
            has_version_tags: false,
        })
    }

    #[test]
    fn layered_repo_drafts_guarded_core_and_free_edge() {
        let files = sig(&[
            "src/domain/order.rs",
            "src/infra/db.rs",
            "src/usecase/place_order.rs",
        ]);
        let d = detect(&files);
        // core 系は guarded・端系は free。
        let domain = d.layers.iter().find(|l| l.name == "domain").unwrap();
        assert_eq!(domain.autonomy, "guarded");
        assert_eq!(domain.paths, vec!["**/domain/**"]);
        let infra = d.layers.iter().find(|l| l.name == "infra").unwrap();
        assert_eq!(infra.autonomy, "free");
    }

    #[test]
    fn core_and_edge_present_drafts_dependency_boundary() {
        let files = sig(&["src/domain/order.rs", "src/infra/db.rs"]);
        let d = detect(&files);
        assert_eq!(d.boundaries.len(), 1);
        let b = &d.boundaries[0];
        // core が端へ依存しない方向境界。forbid に端ディレクトリ名。
        assert!(b.paths.iter().any(|p| p.contains("domain")));
        assert!(b.forbid.iter().any(|f| f.contains("infra")));
        // forbid は妥当な正規表現。
        for f in &b.forbid {
            assert!(regex::Regex::new(f).is_ok());
        }
    }

    #[test]
    fn flat_repo_drafts_no_layers_or_boundary() {
        let files = sig(&["main.py", "utils.py", "README.md"]);
        let d = detect(&files);
        assert!(d.layers.is_empty());
        assert!(d.boundaries.is_empty());
    }

    #[test]
    fn migration_files_draft_irreversible() {
        let files = sig(&["db/migrations/001_init.sql"]);
        let d = detect(&files);
        let m = d
            .irreversible
            .iter()
            .find(|i| i.operation.contains("migration"))
            .expect("migration irreversible draft");
        assert!(regex::Regex::new(&m.detect).is_ok());
    }

    #[test]
    fn terraform_and_k8s_draft_irreversible() {
        let files = sig(&["infra/main.tf", "deploy/k8s/app.yaml"]);
        let d = detect(&files);
        assert!(
            d.irreversible
                .iter()
                .any(|i| i.detect.contains("terraform"))
        );
        assert!(d.irreversible.iter().any(|i| i.detect.contains("kubectl")));
    }

    #[test]
    fn detect_regexes_are_all_valid() {
        // 出力する detect / forbid は全て妥当な正規表現 (壊れた draft を出さない)。
        let files = sig(&[
            "src/domain/x.rs",
            "src/infra/y.rs",
            "db/migrations/1.sql",
            "infra/main.tf",
            "k8s/app.yaml",
        ]);
        let d = detect(&files);
        for i in &d.irreversible {
            assert!(regex::Regex::new(&i.detect).is_ok(), "detect: {}", i.detect);
        }
        for b in &d.boundaries {
            for f in &b.forbid {
                assert!(regex::Regex::new(f).is_ok(), "forbid: {f}");
            }
        }
    }

    #[test]
    fn render_quality_toml_parses_back() {
        // 描いた quality 断片は妥当な TOML として読み戻せる (貼って使える)。
        let files = sig(&["src/domain/x.rs", "src/infra/y.rs"]);
        let d = detect(&files);
        let toml = render_quality_toml(&d);
        assert!(!toml.is_empty());
        let parsed: Result<toml::Table, _> = toml::from_str(&toml);
        assert!(parsed.is_ok(), "rendered quality.toml must parse: {toml}");
    }

    #[test]
    fn render_rules_markdown_has_detect_lines() {
        let files = sig(&["infra/main.tf"]);
        let d = detect(&files);
        let md = render_rules_markdown(&d);
        assert!(md.contains("detect:"));
        assert!(md.contains("terraform"));
    }

    #[test]
    fn empty_when_nothing_detected() {
        let files = sig(&["main.go", "go.mod"]);
        let d = detect(&files);
        assert!(d.is_empty());
        assert!(render_quality_toml(&d).is_empty());
        assert!(render_rules_markdown(&d).is_empty());
    }
}
