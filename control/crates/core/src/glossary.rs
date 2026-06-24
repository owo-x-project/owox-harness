//! 用語集の動的操作。`.owox/glossary.md` を読取・追記し、変更・削除は人間ゲートにする。
//!
//! canon 直読み禁止 (`docs/decisions/20260613-Phase5-スキルと入口.md`) の読み経路。
//! 用語名は session 開始で注入、定義は出現時 push、随時の取得はこの lookup。
//!
//! ゲート: 追加は語彙を増やすだけで低リスクのため AI 可 (来歴へ記録)。
//! 変更・削除は canonical な意味を書き換える・失うため人間判断。open 決定 (gate) を立て、
//! needs_human で止める。人間が canon を編集し gate.approve で解決する (ブランドは人間が握る)。

use std::path::Path;

use serde_json::json;

use crate::envelope::Envelope;
use crate::model::Glossary;
use crate::record::{DecisionLinks, DecisionStatus, RecordInput, record_decision};

/// `.owox/glossary.md` を読む。無ければ空。
fn load(owox_dir: &Path) -> Result<Glossary, String> {
    let path = owox_dir.join("glossary.md");
    match std::fs::read_to_string(&path) {
        Ok(text) => Glossary::from_markdown(&text)
            .map_err(|e| format!("{} を解釈できない: {e}", path.display())),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(Glossary::default()),
        Err(err) => Err(format!("{} を読めない: {err}", path.display())),
    }
}

/// 予約見出し `## Forbidden` の開始バイト位置を返す (無ければ None)。
/// 見出し名は大文字小文字を無視する (パーサの take と揃える)。
fn forbidden_heading_offset(body: &str) -> Option<usize> {
    let mut offset = 0;
    for line in body.split_inclusive('\n') {
        let t = line.trim_end();
        if let Some(rest) = t.strip_prefix("## ")
            && rest.trim().eq_ignore_ascii_case("Forbidden")
        {
            return Some(offset);
        }
        offset += line.len();
    }
    None
}

/// 1 用語あたりの別名の上限。語彙の発散を抑える (人間の手編集は固定層として弾かない)。
const MAX_ALIASES: usize = 5;

/// 用語を大文字小文字を無視して探す。正規名・別名のどちらでも引ける。
fn find_term<'a>(glossary: &'a Glossary, term: &str) -> Option<&'a crate::model::GlossaryEntry> {
    let lower = term.trim().to_lowercase();
    glossary.entries.iter().find(|e| {
        e.term.to_lowercase() == lower || e.aliases.iter().any(|a| a.to_lowercase() == lower)
    })
}

/// glossary.lookup。用語の定義を引く。見つからなければ found=false で返す。
pub fn lookup(owox_dir: &Path, term: &str) -> Envelope {
    let glossary = match load(owox_dir) {
        Ok(g) => g,
        Err(err) => return Envelope::failed(err),
    };
    match find_term(&glossary, term) {
        Some(entry) => Envelope::ok(
            format!("Definition of {}.", entry.term),
            json!({ "found": true, "term": entry.term, "aliases": entry.aliases, "definition": entry.definition }),
        ),
        None => Envelope::ok(
            format!("No project-specific definition for \"{term}\"."),
            json!({ "found": false, "term": term }),
        ),
    }
}

/// glossary.add。用語を追加する (AI 可)。glossary.md へ追記し、来歴へ記録する。
///
/// `term` は `用語 | 別名1 | 別名2` の形で別名を含められる (コロン左をパイプ区切り)。
/// 別名の数は MAX_ALIASES まで。別名は正規名と同義として照合し、床へは出さない。
pub fn add(owox_dir: &Path, today: &str, term: &str, definition: &str) -> Envelope {
    let definition = definition.trim();
    // 用語名をパイプで分け、先頭が正規名・残りが別名。
    let mut parts = term.split('|').map(str::trim).filter(|s| !s.is_empty());
    let Some(canonical) = parts.next() else {
        return Envelope::failed("term が空");
    };
    let aliases: Vec<String> = parts.map(str::to_string).collect();
    if definition.is_empty() {
        return Envelope::failed("definition が空");
    }
    if aliases.len() > MAX_ALIASES {
        return Envelope::failed(format!(
            "別名は 1 用語あたり {MAX_ALIASES} 個まで ({} 個指定された)",
            aliases.len()
        ));
    }

    let glossary = match load(owox_dir) {
        Ok(g) => g,
        Err(err) => return Envelope::failed(err),
    };
    // 正規名・別名のどれかが既存の用語・別名と衝突するなら足さない (意味の二重化を防ぐ)。
    for name in std::iter::once(canonical).chain(aliases.iter().map(String::as_str)) {
        if find_term(&glossary, name).is_some() {
            return Envelope::failed(format!(
                "用語または別名 {name} は既にある。意味を変えるなら canon.propose を使う"
            ));
        }
    }

    let path = owox_dir.join("glossary.md");
    let mut body = std::fs::read_to_string(&path).unwrap_or_default();
    // 見出しが無ければ作る (パーサは `## ` 配下の `- ` を読む)。
    if !body.lines().any(|l| l.trim_start().starts_with("## ")) {
        body = "## Glossary\n\n".to_string();
    } else if !body.ends_with('\n') {
        body.push('\n');
    }
    // 別名つきはパイプ区切りで再構成する。
    let names = if aliases.is_empty() {
        canonical.to_string()
    } else {
        format!("{canonical} | {}", aliases.join(" | "))
    };
    let line = format!("- {names}: {definition}\n");
    // 予約見出し Forbidden の前へ差し込む。末尾追記だと Forbidden が最後の節の時に
    // 追加した用語が禁止語として読まれてしまう (add は通常用語のみ・禁止語は人間が握る)。
    let body = match forbidden_heading_offset(&body) {
        Some(at) => format!("{}{line}{}", &body[..at], &body[at..]),
        None => format!("{body}{line}"),
    };
    if let Err(err) = std::fs::write(&path, body) {
        return Envelope::failed(format!("{} へ書けない: {err}", path.display()));
    }

    let rec = record_decision(
        owox_dir,
        today,
        RecordInput {
            title: format!("Add glossary term {canonical}"),
            status: DecisionStatus::Adopted,
            rationale: format!("Added project term. {names}: {definition}"),
            links: DecisionLinks::default(),
            supersedes: Vec::new(),
        },
    );
    Envelope::ok(
        format!("Added glossary term {canonical}."),
        json!({ "term": canonical, "aliases": aliases, "definition": definition }),
    )
    .with_decision_ids(rec.decision_ids)
}

// 用語の変更・削除は統一モデルの canon.propose (crate::canon) へ移した
// (`docs/decisions/20260614-Phase7-経験IOと二層ルール.md` の追補)。ここは追加と参照だけを担う。

#[cfg(test)]
mod tests {
    use super::*;
    use crate::envelope::Status;
    use std::path::PathBuf;

    fn tempdir() -> PathBuf {
        use std::sync::atomic::{AtomicU32, Ordering};
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let pid = std::process::id();
        let dir = std::env::temp_dir().join(format!("owox-glossary-test-{pid}-{n}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn add_then_lookup() {
        let owox = tempdir();
        assert_eq!(
            add(&owox, "20260613", "canon", "source of truth").status,
            Status::Ok
        );
        let env = lookup(&owox, "Canon"); // 大文字小文字を無視
        let data = env.data.unwrap();
        assert_eq!(data["found"], true);
        assert_eq!(data["definition"], "source of truth");
    }

    #[test]
    fn add_records_decision_and_persists() {
        let owox = tempdir();
        let env = add(&owox, "20260613", "target harness", "generated files");
        assert!(!env.decision_ids.is_empty());
        // glossary.md に追記される。
        let body = std::fs::read_to_string(owox.join("glossary.md")).unwrap();
        assert!(body.contains("- target harness: generated files"));
        assert!(body.contains("## "));
    }

    #[test]
    fn add_inserts_before_forbidden_section() {
        // Forbidden が最後の節でも、追加した用語が禁止語として読まれないこと。
        let owox = tempdir();
        std::fs::write(
            owox.join("glossary.md"),
            "## Glossary\n- canon: x\n\n## Forbidden\n- \\bbad\\b: 禁止\n",
        )
        .unwrap();
        assert_eq!(
            add(&owox, "20260614", "target harness", "y").status,
            Status::Ok
        );
        let g = crate::model::Glossary::from_markdown(
            &std::fs::read_to_string(owox.join("glossary.md")).unwrap(),
        )
        .unwrap();
        // 用語は 2 件、禁止語は 1 件のまま (追加分が Forbidden に混ざらない)。
        assert_eq!(g.entries.len(), 2);
        assert_eq!(g.forbidden.len(), 1);
        assert!(g.entries.iter().any(|e| e.term == "target harness"));
    }

    #[test]
    fn add_with_aliases_then_lookup_by_alias() {
        let owox = tempdir();
        assert_eq!(
            add(
                &owox,
                "20260621",
                "target harness | th | harness output",
                "generated files"
            )
            .status,
            Status::Ok
        );
        // 別名でも正規名でも引ける。返るのは正規名の定義。
        for q in ["target harness", "th", "Harness Output"] {
            let data = lookup(&owox, q).data.unwrap();
            assert_eq!(data["found"], true, "{q} で引けない");
            assert_eq!(data["term"], "target harness");
            assert_eq!(data["definition"], "generated files");
        }
        // glossary.md にパイプ形式で残る。
        let body = std::fs::read_to_string(owox.join("glossary.md")).unwrap();
        assert!(body.contains("- target harness | th | harness output: generated files"));
    }

    #[test]
    fn add_alias_colliding_with_existing_term_fails() {
        let owox = tempdir();
        add(&owox, "20260621", "canon", "source of truth");
        // 別名が既存用語と衝突するなら足さない。
        assert_eq!(
            add(&owox, "20260621", "正本 | canon", "x").status,
            Status::Failed
        );
    }

    #[test]
    fn add_too_many_aliases_fails() {
        let owox = tempdir();
        let env = add(&owox, "20260621", "t | a1 | a2 | a3 | a4 | a5 | a6", "def");
        assert_eq!(env.status, Status::Failed);
    }

    #[test]
    fn add_duplicate_fails() {
        let owox = tempdir();
        add(&owox, "20260613", "canon", "v1");
        assert_eq!(add(&owox, "20260613", "canon", "v2").status, Status::Failed);
    }

    #[test]
    fn lookup_missing_is_ok_not_found() {
        let owox = tempdir();
        let env = lookup(&owox, "nope");
        assert_eq!(env.status, Status::Ok);
        assert_eq!(env.data.unwrap()["found"], false);
    }

    // 変更・削除は canon.propose (crate::canon) のテストで担保する。
}
