//! 成長層 (practices.md) の追記。経験から育つ運用指針を AI が育てる
//! (`docs/decisions/20260614-Phase7-経験IOと二層ルール.md`)。
//!
//! 固定 rules.md と別管理。追記は AI 可 (語彙・指針を増やすだけは低リスク。glossary.add と同じ思想)。
//! 変更・削除は git で人間が編集する。古い指針は鮮度 decay が見直し合図を出す (捨てる根拠にしない)。

use std::path::Path;

use serde_json::json;

use crate::envelope::Envelope;
use crate::record::{DecisionLinks, DecisionStatus, RecordInput, record_decision};

/// practice.lookup。語句の部分一致で指針を引く (大文字小文字を無視)。
///
/// 床が肥大化で縮んだ後でも、床から外れた古い指針を語句で取り出せる読み経路
/// (`docs/decisions/20260621-Phase9-経験層スケールとGitHub連携とkickoff束ね.md`)。
/// query が空なら全件を新しい順で返す。
pub fn lookup(owox_dir: &Path, query: &str) -> Envelope {
    let path = owox_dir.join("practices.md");
    let body = std::fs::read_to_string(&path).unwrap_or_default();
    let practices = match crate::model::Practices::from_markdown(&body) {
        Ok(p) => p,
        Err(err) => return Envelope::failed(format!("{} を解釈できない: {err}", path.display())),
    };

    let q = query.trim().to_lowercase();
    let mut matched: Vec<&crate::model::Practice> = practices
        .entries
        .iter()
        .filter(|p| q.is_empty() || p.text.to_lowercase().contains(&q))
        .collect();
    matched.sort_by(|a, b| b.date.cmp(&a.date));

    let items: Vec<_> = matched
        .iter()
        .map(|p| json!({ "date": p.date, "text": p.text }))
        .collect();
    Envelope::ok(
        format!("Found {} matching practice(s).", items.len()),
        json!({ "found": items.len(), "practices": items }),
    )
}

/// practice.add。成長層 (practices.md) へ指針を 1 件追記し、来歴へ記録する (AI 可)。
pub fn add(owox_dir: &Path, today: &str, text: &str) -> Envelope {
    let text = text.trim();
    if text.is_empty() {
        return Envelope::failed("practice text が空");
    }

    let path = owox_dir.join("practices.md");
    let mut body = std::fs::read_to_string(&path).unwrap_or_default();

    // 完全一致ガード: 同一テキストが既にあれば足さない (重複行・余分な来歴を増やさない)。
    // 言い回し違いの重複は run_practice_redundancy が advisory で拾う。
    if let Ok(existing) = crate::model::Practices::from_markdown(&body)
        && existing.entries.iter().any(|p| p.text.trim() == text)
    {
        return Envelope::ok(
            "That practice is already recorded; not adding a duplicate.",
            json!({ "text": text, "duplicate": true }),
        );
    }

    // 見出しが無ければ作る (パーサは `## ` 配下の `- ` を読む)。
    if !body.lines().any(|l| l.trim_start().starts_with("## ")) {
        body = "# Practices\n\n## Practices\n\n".to_string();
    } else if !body.ends_with('\n') {
        body.push('\n');
    }
    body.push_str(&format!("- {today}: {text}\n"));
    if let Err(err) = std::fs::write(&path, body) {
        return Envelope::failed(format!("{} へ書けない: {err}", path.display()));
    }

    let rec = record_decision(
        owox_dir,
        today,
        RecordInput {
            title: "Add practice".to_string(),
            status: DecisionStatus::Adopted,
            rationale: format!("Grew an operating practice from experience. {text}"),
            links: DecisionLinks::default(),
            supersedes: Vec::new(),
        },
    );
    Envelope::ok("Recorded a practice.", json!({ "text": text }))
        .with_decision_ids(rec.decision_ids)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::envelope::Status;
    use crate::model::Practices;
    use std::path::PathBuf;

    fn tempdir() -> PathBuf {
        use std::sync::atomic::{AtomicU32, Ordering};
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let pid = std::process::id();
        let dir = std::env::temp_dir().join(format!("owox-practices-test-{pid}-{n}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn add_appends_dated_entry_and_records() {
        let owox = tempdir();
        let env = add(&owox, "20260614", "always add a regression test");
        assert_eq!(env.status, Status::Ok);
        assert!(!env.decision_ids.is_empty());
        let body = std::fs::read_to_string(owox.join("practices.md")).unwrap();
        let p = Practices::from_markdown(&body).unwrap();
        assert_eq!(p.entries.len(), 1);
        assert_eq!(p.entries[0].date, "20260614");
        assert_eq!(p.entries[0].text, "always add a regression test");
    }

    #[test]
    fn empty_text_fails() {
        let owox = tempdir();
        assert_eq!(add(&owox, "20260614", "  ").status, Status::Failed);
    }

    #[test]
    fn lookup_matches_by_keyword_newest_first() {
        let owox = tempdir();
        add(&owox, "20260610", "prefer small diffs");
        add(&owox, "20260620", "always add a regression test");
        // 語句一致 (大小無視)。
        let env = lookup(&owox, "REGRESSION");
        let data = env.data.unwrap();
        assert_eq!(data["found"], 1);
        assert_eq!(data["practices"][0]["text"], "always add a regression test");
        // 空 query は全件・新しい順。
        let all = lookup(&owox, "  ").data.unwrap();
        assert_eq!(all["found"], 2);
        assert_eq!(all["practices"][0]["date"], "20260620");
    }

    #[test]
    fn lookup_missing_file_is_ok_empty() {
        let owox = tempdir();
        let env = lookup(&owox, "anything");
        assert_eq!(env.status, Status::Ok);
        assert_eq!(env.data.unwrap()["found"], 0);
    }

    #[test]
    fn exact_duplicate_is_not_added() {
        let owox = tempdir();
        add(&owox, "20260614", "prefer small diffs");
        // 完全一致は別日でも足さない (ok だが duplicate フラグ)。
        let env = add(&owox, "20260615", "prefer small diffs");
        assert_eq!(env.status, Status::Ok);
        assert_eq!(env.data.unwrap()["duplicate"], serde_json::json!(true));
        assert!(env.decision_ids.is_empty()); // 来歴も増やさない
        let p =
            Practices::from_markdown(&std::fs::read_to_string(owox.join("practices.md")).unwrap())
                .unwrap();
        assert_eq!(p.entries.len(), 1);
    }
}
