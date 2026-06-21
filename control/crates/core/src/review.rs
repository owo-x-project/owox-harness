//! 判断型レビューの枠組み (`docs/decisions/20260614-Phase6-レビュー枠組み.md`)。
//!
//! 観点 (lens) を owox 標準のベストプラクティスとして持ち、変更内容で機械選択する (routable)。
//! 各観点は独立に走る離散単位で、適用条件 (Always か Paths glob) を持つ。
//! 進め方の枠組みは review 入口 skill の本文が持ち、実際のレビューは AI が回す。
//!
//! 枠組みは実行モデル非依存: 本スライスは単一エージェントが順に回し、Phase8 のルータが
//! 同じ select_lenses 契約を読んで観点ごとに subagent を立て、ティアと数を機械的に振り分ける。
//!
//! 変更ファイルは呼び出し側 (mcp) が git diff で集めて渡す (core は git を持たず決定論)。

use std::path::Path;

use serde_json::json;

use crate::envelope::Envelope;
use crate::quality::glob_to_regex;

/// 観点の適用条件。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Applicability {
    /// どの変更にも常に適用する (普遍観点)。
    Always,
    /// 変更が触ったファイルがこの glob に当たる時だけ適用する。
    Paths(Vec<String>),
}

/// レビュー観点 1 件。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lens {
    pub id: String,
    pub description: String,
    pub when: Applicability,
}

impl Lens {
    /// 変更ファイル群にこの観点が適用されるか。
    fn applies(&self, changed_files: &[String]) -> bool {
        match &self.when {
            Applicability::Always => true,
            Applicability::Paths(globs) => globs.iter().any(|g| {
                let re = glob_to_regex(g);
                changed_files.iter().any(|f| re.is_match(f))
            }),
        }
    }
}

/// owox 標準の観点 (ベストプラクティス)。普遍観点は Always、依存は依存マニフェスト変更で適用。
///
/// 言語横断で既知のマニフェスト名だけをトリガにする (owox は言語非依存)。
fn standard_lenses() -> Vec<Lens> {
    let always: &[(&str, &str)] = &[
        (
            "correctness",
            "Does the change do what it intends, including edge cases and failure paths?",
        ),
        (
            "design",
            "Does it fit the responsibility split, dependency direction, and layer boundaries?",
        ),
        (
            "security",
            "Secrets, dangerous operations, external calls, and regression risk.",
        ),
        (
            "plan-alignment",
            "Does it follow existing naming, structure, and conventions instead of reinventing them?",
        ),
        (
            "requirement",
            "Does the change tie back to a requirement and its acceptance criteria?",
        ),
        (
            "pruning",
            "What did the change leave unnecessary: dead code, unused items, duplication, or scaffolding to prune?",
        ),
    ];

    let mut lenses: Vec<Lens> = always
        .iter()
        .map(|(id, desc)| Lens {
            id: id.to_string(),
            description: desc.to_string(),
            when: Applicability::Always,
        })
        .collect();

    // 依存マニフェストが変わった時だけ効く条件観点 (routable の既定例)。
    lenses.push(Lens {
        id: "dependency".to_string(),
        description: "A dependency manifest changed: review the new dependency's justification, alternatives, and policy.".to_string(),
        when: Applicability::Paths(
            [
                "**/Cargo.toml",
                "**/Cargo.lock",
                "**/package.json",
                "**/package-lock.json",
                "**/go.mod",
                "**/go.sum",
                "**/pyproject.toml",
                "**/requirements.txt",
                "**/Gemfile",
                "**/Gemfile.lock",
                "**/pom.xml",
                "**/build.gradle",
            ]
            .iter()
            .map(|s| s.to_string())
            .collect(),
        ),
    });

    lenses
}

/// 観点を読む。owox 標準に、プロジェクトの review.toml を上書き・追加する (commands と同方針)。
pub fn load_lenses(owox_dir: &Path) -> Result<Vec<Lens>, String> {
    let mut lenses = standard_lenses();
    let path = owox_dir.join("review.toml");
    match std::fs::read_to_string(&path) {
        Ok(text) => {
            for lens in parse_review_toml(&text)
                .map_err(|e| format!("{} を解釈できない: {e}", path.display()))?
            {
                upsert(&mut lenses, lens);
            }
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => return Err(format!("{} を読めない: {err}", path.display())),
    }
    Ok(lenses)
}

/// 同 id があれば差し替え、無ければ足す (プロジェクトが標準を上書き・拡張できる)。
fn upsert(lenses: &mut Vec<Lens>, lens: Lens) {
    if let Some(slot) = lenses.iter_mut().find(|l| l.id == lens.id) {
        *slot = lens;
    } else {
        lenses.push(lens);
    }
}

/// review.toml を読む。`[[lens]]` の並び。paths があれば Paths、無ければ Always。未知キーは弾く。
fn parse_review_toml(text: &str) -> Result<Vec<Lens>, String> {
    #[derive(serde::Deserialize)]
    struct Raw {
        #[serde(default)]
        lens: Vec<LensRaw>,
    }
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct LensRaw {
        id: String,
        description: String,
        #[serde(default)]
        paths: Vec<String>,
    }
    let raw: Raw = toml::from_str(text).map_err(|e| e.to_string())?;
    Ok(raw
        .lens
        .into_iter()
        .map(|l| Lens {
            id: l.id,
            description: l.description,
            when: if l.paths.is_empty() {
                Applicability::Always
            } else {
                Applicability::Paths(l.paths)
            },
        })
        .collect())
}

/// 変更ファイルに適用される観点を機械選択する。Phase8 のルータも同じ契約を読む。
pub fn select_lenses(lenses: &[Lens], changed_files: &[String]) -> Vec<Lens> {
    lenses
        .iter()
        .filter(|l| l.applies(changed_files))
        .cloned()
        .collect()
}

/// review.lenses tool。今の変更に適用される観点を機械選択して封筒で返す。
pub fn review_lenses_envelope(owox_dir: &Path, changed_files: &[String]) -> Envelope {
    let lenses = match load_lenses(owox_dir) {
        Ok(l) => l,
        Err(err) => return Envelope::failed(err),
    };
    let selected = select_lenses(&lenses, changed_files);
    let list: Vec<_> = selected
        .iter()
        .map(|l| json!({ "id": l.id, "description": l.description }))
        .collect();
    Envelope::ok(
        format!(
            "{} review perspective(s) apply to {} changed file(s).",
            list.len(),
            changed_files.len()
        ),
        json!({ "lenses": list, "changed_files": changed_files.len() }),
    )
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
        let dir = std::env::temp_dir().join(format!("owox-review-test-{pid}-{n}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn ids(lenses: &[Lens]) -> Vec<String> {
        lenses.iter().map(|l| l.id.clone()).collect()
    }

    #[test]
    fn standard_has_the_six_universal_lenses() {
        let names = ids(&standard_lenses());
        for expected in [
            "correctness",
            "design",
            "security",
            "plan-alignment",
            "requirement",
            "pruning",
        ] {
            assert!(names.contains(&expected.to_string()), "missing {expected}");
        }
    }

    #[test]
    fn always_lenses_apply_to_any_change() {
        let lenses = standard_lenses();
        let selected = ids(&select_lenses(&lenses, &["src/main.rs".to_string()]));
        // 普遍観点は常に選ばれる。
        assert!(selected.contains(&"correctness".to_string()));
        assert!(selected.contains(&"pruning".to_string()));
        // 依存マニフェストは変わっていないので dependency は選ばれない。
        assert!(!selected.contains(&"dependency".to_string()));
    }

    #[test]
    fn dependency_lens_fires_on_manifest_change() {
        let lenses = standard_lenses();
        let selected = ids(&select_lenses(
            &lenses,
            &["src/main.rs".to_string(), "Cargo.toml".to_string()],
        ));
        assert!(selected.contains(&"dependency".to_string()));
    }

    #[test]
    fn project_lens_adds_and_overrides() {
        let owox = tempdir();
        std::fs::write(
            owox.join("review.toml"),
            "[[lens]]\nid = \"migration-safety\"\ndescription = \"db migrations\"\npaths = [\"migrations/**\"]\n\n[[lens]]\nid = \"security\"\ndescription = \"overridden\"\npaths = [\"src/auth/**\"]\n",
        )
        .unwrap();
        let lenses = load_lenses(&owox).unwrap();
        // 追加された条件観点。
        let mig = lenses.iter().find(|l| l.id == "migration-safety").unwrap();
        assert_eq!(
            mig.when,
            Applicability::Paths(vec!["migrations/**".to_string()])
        );
        // 標準 security を Paths スコープへ上書き。
        let sec = lenses.iter().find(|l| l.id == "security").unwrap();
        assert_eq!(
            sec.when,
            Applicability::Paths(vec!["src/auth/**".to_string()])
        );

        // auth に触れない変更では上書きした security は外れる。
        let selected = ids(&select_lenses(&lenses, &["src/util.rs".to_string()]));
        assert!(!selected.contains(&"security".to_string()));
        assert!(!selected.contains(&"migration-safety".to_string()));
        // migrations を触ると専用観点が効く。
        let selected = ids(&select_lenses(&lenses, &["migrations/001.sql".to_string()]));
        assert!(selected.contains(&"migration-safety".to_string()));
    }

    #[test]
    fn missing_review_toml_yields_standard_only() {
        let owox = tempdir();
        assert_eq!(load_lenses(&owox).unwrap().len(), standard_lenses().len());
    }

    #[test]
    fn envelope_reports_applicable_lenses() {
        let owox = tempdir();
        let env = review_lenses_envelope(&owox, &["Cargo.toml".to_string()]);
        let data = env.data.unwrap();
        let lenses = data["lenses"].as_array().unwrap();
        let names: Vec<_> = lenses.iter().map(|l| l["id"].as_str().unwrap()).collect();
        assert!(names.contains(&"dependency"));
        assert!(names.contains(&"correctness"));
    }
}
