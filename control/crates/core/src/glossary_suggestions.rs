//! 未登録用語の候補を advisory として出す。
//!
//! 生本文は保存せず、その場で抽出した語だけ集計する。

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use regex::Regex;

use crate::model::Glossary;

/// 走査対象のファイルと本文。本文は候補抽出用で、返却本文にはしない。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlossaryScanText {
    pub path: String,
    pub text: String,
}

/// 候補集計用の 1 sighting。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct GlossaryTermHit {
    pub term: String,
    pub source: String,
    pub example: String,
}

/// 用語候補 1 件。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlossarySuggestion {
    pub term: String,
    /// 候補になった根拠。
    pub reason: String,
    /// 候補の存在を示す例。本文ではなくパスや検索語だけ返す。
    pub examples: Vec<String>,
}

#[derive(Default)]
struct Counts {
    display: String,
    occurrences: usize,
    lookup_misses: usize,
    files: BTreeSet<String>,
    sources: BTreeSet<String>,
    examples: BTreeSet<String>,
}

const MAX_TEXT_CHARS: usize = 8_000;
const MAX_SUGGESTIONS: usize = 10;
const MIN_TERM_CHARS: usize = 4;

/// 既存 glossary にない、候補になりそうな用語を返す。
pub fn suggest_terms(owox_dir: &Path, texts: &[GlossaryScanText]) -> Vec<GlossarySuggestion> {
    suggest_terms_from_hits(owox_dir, &extract_term_hits(texts, "scanned text"))
}

/// 走査本文から候補集計用の sighting を作る。
pub fn extract_term_hits(texts: &[GlossaryScanText], source: &str) -> Vec<GlossaryTermHit> {
    let mut hits = Vec::new();
    for text in texts {
        hits.extend(extract_term_hits_from_text(&text.path, &text.text, source));
    }
    hits
}

/// 抽出済み sighting から候補を返す。
pub fn suggest_terms_from_hits(
    owox_dir: &Path,
    hits: &[GlossaryTermHit],
) -> Vec<GlossarySuggestion> {
    let glossary = load_glossary(owox_dir);
    let existing = glossary_names(&glossary);
    let mut counts: BTreeMap<String, Counts> = BTreeMap::new();

    for hit in hits {
        let key = normalized_name(&hit.term);
        if key.is_empty() || existing.contains(&key) {
            continue;
        }
        let entry = counts.entry(key).or_default();
        if entry.display.is_empty() {
            entry.display = hit.term.clone();
        }
        entry.occurrences += 1;
        if hit.source == "lookup miss" {
            entry.lookup_misses += 1;
        }
        entry.sources.insert(hit.source.clone());
        if looks_like_path(&hit.example) {
            entry.files.insert(hit.example.clone());
        }
        if !hit.example.trim().is_empty() {
            entry.examples.insert(hit.example.clone());
        }
    }

    let mut out: Vec<(usize, usize, usize, GlossarySuggestion)> = counts
        .into_values()
        .filter_map(|counts| {
            if counts.occurrences < 2 && counts.lookup_misses == 0 && counts.files.len() < 2 {
                return None;
            }
            let mut reasons = Vec::new();
            if !counts.files.is_empty() {
                reasons.push(format!("{} files", counts.files.len()));
            }
            reasons.push(format!("{} occurrences", counts.occurrences));
            if counts.lookup_misses > 0 {
                reasons.push(format!("lookup miss {}x", counts.lookup_misses));
            }
            let non_default_sources: Vec<_> = counts
                .sources
                .iter()
                .filter(|source| {
                    !matches!(
                        source.as_str(),
                        "changed file" | "scanned text" | "read scan" | "user prompt"
                    )
                })
                .cloned()
                .collect();
            if !non_default_sources.is_empty() {
                reasons.push(non_default_sources.join(" + "));
            }
            let mut examples: Vec<String> = counts.examples.into_iter().take(3).collect();
            examples.sort();
            Some((
                counts.files.len(),
                counts.occurrences,
                counts.lookup_misses,
                GlossarySuggestion {
                    term: counts.display,
                    reason: reasons.join(", "),
                    examples,
                },
            ))
        })
        .collect();

    let phrase_parts: BTreeSet<String> = out
        .iter()
        .filter(|(_, _, _, suggestion)| suggestion.term.contains(' '))
        .flat_map(|(_, _, _, suggestion)| {
            normalized_name(&suggestion.term)
                .split_whitespace()
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .collect();
    out.retain(|(_, _, _, suggestion)| {
        suggestion.term.contains(' ') || !phrase_parts.contains(&normalized_name(&suggestion.term))
    });

    out.sort_by(|a, b| {
        b.0.cmp(&a.0)
            .then_with(|| b.1.cmp(&a.1))
            .then_with(|| b.2.cmp(&a.2))
            .then_with(|| a.3.term.cmp(&b.3.term))
    });

    out.into_iter()
        .take(MAX_SUGGESTIONS)
        .map(|(_, _, _, suggestion)| suggestion)
        .collect()
}

fn extract_term_hits_from_text(path: &str, text: &str, source: &str) -> Vec<GlossaryTermHit> {
    let text = text.chars().take(MAX_TEXT_CHARS).collect::<String>();
    let re = Regex::new(
        r"(?ix)\b[A-Za-z][A-Za-z0-9]*(?:[._-][A-Za-z0-9]+)*\b|[\p{Han}\p{Hiragana}\p{Katakana}]{4,30}\b",
    )
    .unwrap();
    let mut hits = Vec::new();
    for line in text.lines() {
        let mut seen = BTreeSet::new();
        // phrase_tokens: フレーズ組み立て用。normalize_token の結果に依らず全トークンを収集。
        let phrase_tokens: Vec<String> =
            re.find_iter(line).map(|m| m.as_str().to_string()).collect();
        for raw in &phrase_tokens {
            let Some(term) = normalize_token(raw) else {
                continue;
            };
            if !seen.insert(term.clone()) {
                continue;
            }
            hits.push(GlossaryTermHit {
                term,
                source: source.to_string(),
                example: path.to_string(),
            });
        }
        for pair in phrase_tokens.windows(2) {
            let phrase = candidate_phrase(&pair[0], &pair[1]);
            let Some(term) = phrase.and_then(|phrase| normalize_token(&phrase)) else {
                continue;
            };
            if !seen.insert(term.clone()) {
                continue;
            }
            hits.push(GlossaryTermHit {
                term,
                source: source.to_string(),
                example: path.to_string(),
            });
        }
    }
    hits
}

fn candidate_phrase(first: &str, second: &str) -> Option<String> {
    let first_char_upper = first
        .chars()
        .next()
        .map(|ch| ch.is_ascii_uppercase())
        .unwrap_or(false);
    if !first_char_upper {
        return None;
    }
    Some(format!("{first} {second}"))
}

fn load_glossary(owox_dir: &Path) -> Glossary {
    let path = owox_dir.join("glossary.md");
    match std::fs::read_to_string(&path) {
        Ok(text) => Glossary::from_markdown(&text).unwrap_or_default(),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Glossary::default(),
        Err(_) => Glossary::default(),
    }
}

fn glossary_names(glossary: &Glossary) -> BTreeSet<String> {
    glossary
        .entries
        .iter()
        .flat_map(|entry| {
            std::iter::once(&entry.term)
                .chain(entry.aliases.iter())
                .map(|name| normalized_name(name))
        })
        .collect()
}

fn normalized_name(name: &str) -> String {
    name.trim()
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric() && c != '_')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

fn normalize_token(token: &str) -> Option<String> {
    let trimmed = token.trim_matches(|c: char| c.is_ascii_punctuation() || c == ':' || c == ';');
    if trimmed.len() < MIN_TERM_CHARS || trimmed.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    if trimmed.contains('/') || trimmed.contains('\\') {
        return None;
    }
    let lower = trimmed.to_ascii_lowercase();
    if lower.starts_with("owox:") || lower.ends_with(".owox") {
        return None;
    }
    if is_reserved_word(&lower) || is_generic_word(&lower) {
        return None;
    }
    if is_file_extension_token(&lower) {
        return None;
    }
    if is_path_like(&lower) {
        return None;
    }
    if !is_camel_token(trimmed)
        && !is_lower_snake_or_kebab(trimmed)
        && trimmed.chars().all(|c| c.is_ascii_alphabetic())
    {
        return None;
    }
    Some(trimmed.to_string())
}

fn looks_like_path(example: &str) -> bool {
    example.contains('/') || example.contains('\\')
}

fn is_reserved_word(lower: &str) -> bool {
    const RESERVED: &[&str] = &[
        "use",
        "let",
        "fn",
        "if",
        "else",
        "for",
        "while",
        "loop",
        "match",
        "struct",
        "enum",
        "impl",
        "trait",
        "pub",
        "mod",
        "crate",
        "self",
        "super",
        "true",
        "false",
        "null",
        "def",
        "class",
        "import",
        "from",
        "return",
        "print",
        "read",
        "write",
        "delete",
        "commit",
        "review",
        "verify",
        "canon",
        "glossary",
        "rules",
        "practice",
        "task",
        "next",
        "mission",
        "phase",
        "initial",
        "stable",
        "maintenance",
    ];
    RESERVED.contains(&lower)
}

fn is_generic_word(lower: &str) -> bool {
    const GENERIC: &[&str] = &[
        "project", "repo", "file", "files", "tool", "tools", "task", "work", "change", "changes",
        "code", "docs", "read", "write", "add", "remove", "delete", "create", "update", "list",
        "get", "set", "start", "finish", "verify", "run", "scope", "section", "current", "next",
        "before", "after", "when", "then", "given", "human", "ai", "mission", "phase",
    ];
    GENERIC.contains(&lower)
}

fn is_file_extension_token(lower: &str) -> bool {
    const EXTENSIONS: &[&str] = &[
        "rs", "ts", "tsx", "js", "jsx", "py", "go", "java", "sh", "toml", "json", "yml", "yaml",
        "md", "txt", "css", "html", "svg", "png", "jpg", "jpeg", "gif", "lock",
    ];
    EXTENSIONS.contains(&lower)
}

fn is_path_like(lower: &str) -> bool {
    lower.contains('/')
        || lower.contains('\\')
        || lower.ends_with(".rs")
        || lower.ends_with(".ts")
        || lower.ends_with(".js")
        || lower.ends_with(".py")
        || lower.ends_with(".go")
}

fn is_camel_token(token: &str) -> bool {
    token.chars().any(|c| c.is_ascii_uppercase())
}

fn is_lower_snake_or_kebab(token: &str) -> bool {
    token
        .chars()
        .all(|c| c.is_ascii_lowercase() || c == '_' || c == '-')
        && (token.contains('_') || token.contains('-'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suggests_repeated_project_specific_terms_only() {
        let owox = std::env::temp_dir().join("owox-glossary-suggestions-test");
        let _ = std::fs::remove_dir_all(&owox);
        std::fs::create_dir_all(&owox).unwrap();
        std::fs::write(
            owox.join("glossary.md"),
            "## Glossary\n- canon: source of truth\n",
        )
        .unwrap();
        let suggestions = suggest_terms(
            &owox,
            &[
                GlossaryScanText {
                    path: "docs/requirements/a.md".to_string(),
                    text: "## Target harness\nUse target harness terms. target harness appears twice.\n"
                        .to_string(),
                },
                GlossaryScanText {
                    path: "docs/requirements/b.md".to_string(),
                    text: "Target harness is a key phrase again.\n".to_string(),
                },
                GlossaryScanText {
                    path: "src/main.rs".to_string(),
                    text: "pub fn main() {}\n".to_string(),
                },
            ],
        );
        assert_eq!(suggestions.len(), 1);
        assert_eq!(suggestions[0].term, "Target harness");
        assert!(suggestions[0].reason.contains("2 files"));
        assert_eq!(suggestions[0].examples.len(), 2);
    }

    #[test]
    fn skips_existing_terms_and_aliases() {
        let owox = std::env::temp_dir().join("owox-glossary-suggestions-alias-test");
        let _ = std::fs::remove_dir_all(&owox);
        std::fs::create_dir_all(&owox).unwrap();
        std::fs::write(
            owox.join("glossary.md"),
            "## Glossary\n- target harness | th: generated files\n",
        )
        .unwrap();
        let suggestions = suggest_terms(
            &owox,
            &[GlossaryScanText {
                path: "docs/a.md".to_string(),
                text: "## th\nUse th and target harness in both lines.\n".to_string(),
            }],
        );
        assert!(suggestions.is_empty());
    }

    #[test]
    fn camel_and_snake_tokens_are_candidate_eligible() {
        let owox = std::env::temp_dir().join("owox-glossary-suggestions-camel-test");
        let _ = std::fs::remove_dir_all(&owox);
        std::fs::create_dir_all(&owox).unwrap();
        let suggestions = suggest_terms(
            &owox,
            &[
                GlossaryScanText {
                    path: "docs/a.md".to_string(),
                    text: "TargetHarness is used here for testing.\n".to_string(),
                },
                GlossaryScanText {
                    path: "docs/b.md".to_string(),
                    text: "TargetHarness appears again in this document.\n".to_string(),
                },
            ],
        );
        assert!(
            suggestions.iter().any(|s| s.term == "TargetHarness"),
            "TargetHarness should be a candidate, got: {:?}",
            suggestions.iter().map(|s| &s.term).collect::<Vec<_>>()
        );
    }

    #[test]
    fn snake_case_token_is_candidate_eligible() {
        let owox = std::env::temp_dir().join("owox-glossary-suggestions-snake-test");
        let _ = std::fs::remove_dir_all(&owox);
        std::fs::create_dir_all(&owox).unwrap();
        let suggestions = suggest_terms(
            &owox,
            &[
                GlossaryScanText {
                    path: "crates/a.rs".to_string(),
                    text: "let x = owox_session_cache;\nuse owox_session_cache;\n".to_string(),
                },
                GlossaryScanText {
                    path: "crates/b.rs".to_string(),
                    text: "fn owox_session_cache() {}\n".to_string(),
                },
            ],
        );
        assert!(
            suggestions
                .iter()
                .any(|s| s.term.contains("owox_session_cache")),
            "owox_session_cache should be a candidate, got: {:?}",
            suggestions.iter().map(|s| &s.term).collect::<Vec<_>>()
        );
    }

    #[test]
    fn lookup_miss_can_create_a_candidate() {
        let owox = std::env::temp_dir().join("owox-glossary-suggestions-lookup-test");
        let _ = std::fs::remove_dir_all(&owox);
        std::fs::create_dir_all(&owox).unwrap();
        let suggestions = suggest_terms_from_hits(
            &owox,
            &[
                GlossaryTermHit {
                    term: "TargetHarness".to_string(),
                    source: "lookup miss".to_string(),
                    example: "TargetHarness".to_string(),
                },
                GlossaryTermHit {
                    term: "TargetHarness".to_string(),
                    source: "user prompt".to_string(),
                    example: "user prompt".to_string(),
                },
            ],
        );
        assert_eq!(suggestions.len(), 1);
        assert!(suggestions[0].reason.contains("lookup miss 1x"));
    }
}
