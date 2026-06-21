//! 調査知識層: `.owox/knowledge/<id>.md` に 1 調査 1 ファイル。
//!
//! 記録層 (`record.rs`) を手本に、調査 (リサーチ) の成果を検証つきで蓄積する
//! (`docs/decisions/20260616-Phase8-調査知識層.md`)。指針 (practices) と違い構造化・
//! オンデマンド読み・経過日数鮮度で性質が違うため別層にする。
//!
//! ID は 日付+slug (record.rs と同形式・slugify/allocate_id を共用)。更新は supersede 専用
//! (in-place 更新しない・調査時点で何を知っていたかを残す)。読みは tool 経由のみ (canon 直読み禁止)。
//! 鮮度は decay.rs の run_knowledge_decay が経過日数で機械判定する。

use std::path::{Path, PathBuf};

use serde_json::json;

use crate::envelope::Envelope;
use crate::markdown::Doc;
use crate::record::{allocate_id, slugify};
use crate::secret;

/// 調査知識の状態。supersede 専用なので Draft は持たない。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KnowledgeStatus {
    Current,
    Superseded,
}

impl KnowledgeStatus {
    /// 文字列から状態を読む。tool 引数の検証にも使う。
    pub fn parse(value: &str) -> Result<KnowledgeStatus, String> {
        match value.trim() {
            "current" => Ok(KnowledgeStatus::Current),
            "superseded" => Ok(KnowledgeStatus::Superseded),
            other => Err(format!("status は current / superseded のみ: {other}")),
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            KnowledgeStatus::Current => "current",
            KnowledgeStatus::Superseded => "superseded",
        }
    }
}

/// 調査知識 1 件。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Knowledge {
    pub id: String,
    pub title: String,
    /// 調査日 (YYYYMMDD)。鮮度判定の根拠。
    pub researched_on: String,
    /// 出典の並び (URL か参照)。
    pub sources: Vec<String>,
    /// 調査結果の散文。
    pub summary: String,
    /// 任意のタグ (lookup 補助)。
    pub tags: Vec<String>,
    pub status: KnowledgeStatus,
    /// 置き換えた旧 ID の並び。
    pub supersedes: Vec<String>,
}

/// 調査知識追加の入力。knowledge.add tool が受ける。
#[derive(Debug, Clone)]
pub struct KnowledgeInput {
    pub title: String,
    pub summary: String,
    pub sources: Vec<String>,
    /// 省略時は today (mcp が供給)。
    pub researched_on: Option<String>,
    pub tags: Vec<String>,
    pub supersedes: Vec<String>,
}

impl Knowledge {
    /// Markdown へ描画する。1 行目の `# title` は人間向けタイトル。
    fn render(&self) -> String {
        let mut out = format!("# {}\n\n", self.title);
        out.push_str(&format!("## Researched on\n\n{}\n\n", self.researched_on));

        if !self.sources.is_empty() {
            out.push_str("## Sources\n\n");
            for s in &self.sources {
                out.push_str(&format!("- {s}\n"));
            }
            out.push('\n');
        }

        if !self.summary.trim().is_empty() {
            out.push_str(&format!("## Summary\n\n{}\n\n", self.summary.trim()));
        }

        if !self.tags.is_empty() {
            out.push_str("## Tags\n\n");
            for t in &self.tags {
                out.push_str(&format!("- {t}\n"));
            }
            out.push('\n');
        }

        out.push_str(&format!("## Status\n\n{}\n\n", self.status.as_str()));

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
    fn parse(id: &str, text: &str) -> Result<Knowledge, String> {
        let title = text
            .lines()
            .map(str::trim)
            .find_map(|l| l.strip_prefix("# ").filter(|_| !l.starts_with("## ")))
            .unwrap_or("")
            .trim()
            .to_string();

        let mut doc = Doc::parse(text);

        let researched_on = doc
            .take("Researched on")
            .map(|s| s.text())
            .ok_or_else(|| "Researched on セクションが必須".to_string())?;

        let sources = doc.take("Sources").map(|s| s.list()).unwrap_or_default();
        let summary = doc.take("Summary").map(|s| s.text()).unwrap_or_default();
        let tags = doc.take("Tags").map(|s| s.list()).unwrap_or_default();

        let status = doc
            .take("Status")
            .map(|s| s.text())
            .ok_or_else(|| "Status セクションが必須".to_string())
            .and_then(|t| KnowledgeStatus::parse(&t))?;

        let supersedes = doc.take("Supersedes").map(|s| s.list()).unwrap_or_default();

        Ok(Knowledge {
            id: id.to_string(),
            title,
            researched_on,
            sources,
            summary,
            tags,
            status,
            supersedes,
        })
    }
}

/// `.owox/knowledge/`。
fn knowledge_dir(owox_dir: &Path) -> PathBuf {
    owox_dir.join("knowledge")
}

/// 全調査知識を読む。`.owox/knowledge/*.md`。ディレクトリが無ければ空。
pub fn list_knowledge(owox_dir: &Path) -> Result<Vec<Knowledge>, String> {
    let dir = knowledge_dir(owox_dir);
    let read = match std::fs::read_dir(&dir) {
        Ok(read) => read,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(format!("{} を読めない: {err}", dir.display())),
    };

    let mut items = Vec::new();
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
        let k = Knowledge::parse(&id, &text)
            .map_err(|e| format!("{} を解釈できない: {e}", path.display()))?;
        items.push(k);
    }
    items.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(items)
}

/// 調査知識を 1 件読む。
fn load_knowledge(owox_dir: &Path, id: &str) -> Result<Knowledge, String> {
    let path = knowledge_dir(owox_dir).join(format!("{id}.md"));
    let text = std::fs::read_to_string(&path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            format!("調査知識が無い: {id}")
        } else {
            format!("{} を読めない: {e}", path.display())
        }
    })?;
    Knowledge::parse(id, &text)
}

/// knowledge.add。調査知識を `.owox/knowledge/<id>.md` へ書き、封筒で返す。
///
/// summary + sources を秘密走査し、検出時は failed (書かない)。supersedes 指定時は旧エントリを
/// superseded へ書き換え、新規を current で書く (supersede の実体・専用 tool を増やさない)。
/// today は呼び出し側 (mcp) が与える `YYYYMMDD` (core は時計を読まない)。
pub fn add_knowledge(owox_dir: &Path, today: &str, input: KnowledgeInput) -> Envelope {
    if input.title.trim().is_empty() {
        return Envelope::failed("title が空");
    }
    if input.summary.trim().is_empty() {
        return Envelope::failed("summary が空 (調査結果の要約は必須)");
    }

    // 外部内容を保持するため入口で秘密走査する (summary + sources)。
    let mut scan_text = input.summary.clone();
    for s in &input.sources {
        scan_text.push('\n');
        scan_text.push_str(s);
    }
    let hits = secret::scan(&scan_text);
    if !hits.is_empty() {
        let ids: Vec<String> = hits.iter().map(|f| f.id.clone()).collect();
        return Envelope::failed(format!(
            "Secrets detected in summary or sources; not recording. Remove them first. [{}]",
            ids.join(", ")
        ));
    }

    let researched_on = input
        .researched_on
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(today)
        .to_string();

    let dir = knowledge_dir(owox_dir);
    if let Err(err) = std::fs::create_dir_all(&dir) {
        return Envelope::failed(format!("{} を作れない: {err}", dir.display()));
    }

    // supersedes 指定の旧エントリを superseded へ書き換える (在れば)。
    let mut superseded = Vec::new();
    for old_id in &input.supersedes {
        match load_knowledge(owox_dir, old_id) {
            Ok(mut old) => {
                if old.status != KnowledgeStatus::Superseded {
                    old.status = KnowledgeStatus::Superseded;
                    let path = dir.join(format!("{old_id}.md"));
                    if let Err(err) = std::fs::write(&path, old.render()) {
                        return Envelope::failed(format!("{} へ書けない: {err}", path.display()));
                    }
                }
                superseded.push(old_id.clone());
            }
            Err(err) => return Envelope::failed(err),
        }
    }

    let id = allocate_id(&dir, today, &slugify(&input.title));
    let knowledge = Knowledge {
        id: id.clone(),
        title: input.title,
        researched_on,
        sources: input.sources,
        summary: input.summary,
        tags: input.tags,
        status: KnowledgeStatus::Current,
        supersedes: superseded.clone(),
    };

    let path = dir.join(format!("{id}.md"));
    if let Err(err) = std::fs::write(&path, knowledge.render()) {
        return Envelope::failed(format!("{} へ書けない: {err}", path.display()));
    }

    let reason = if superseded.is_empty() {
        "Recorded the research knowledge.".to_string()
    } else {
        format!(
            "Recorded the research knowledge and superseded {} prior entry(ies).",
            superseded.len()
        )
    };
    Envelope::ok(reason, json!({ "id": id, "superseded": superseded }))
}

/// knowledge.get。調査知識を 1 件全文で返す (canon 直読み禁止の読み口)。
pub fn get_knowledge(owox_dir: &Path, id: &str) -> Envelope {
    match load_knowledge(owox_dir, id) {
        Ok(k) => Envelope::ok(
            format!("Knowledge {id}."),
            json!({
                "id": k.id,
                "title": k.title,
                "researched_on": k.researched_on,
                "sources": k.sources,
                "summary": k.summary,
                "tags": k.tags,
                "status": k.status.as_str(),
                "supersedes": k.supersedes,
            }),
        ),
        Err(err) => Envelope::failed(err),
    }
}

/// knowledge.list。状態・鮮度で絞り、要約せず一覧の概要を返す。
///
/// stale 判定は today と stale_days で行う (current かつ経過日数超え)。stale_only で stale のみ。
pub fn list_knowledge_envelope(
    owox_dir: &Path,
    status: Option<&str>,
    stale_only: bool,
    today: &str,
    stale_days: u32,
) -> Envelope {
    let want_status = match status.map(KnowledgeStatus::parse).transpose() {
        Ok(s) => s,
        Err(err) => return Envelope::failed(err),
    };
    let all = match list_knowledge(owox_dir) {
        Ok(a) => a,
        Err(err) => return Envelope::failed(err),
    };
    let items: Vec<_> = all
        .iter()
        .filter(|k| want_status.is_none_or(|s| k.status == s))
        .map(|k| {
            let stale = is_stale(k, today, stale_days);
            json!({
                "id": k.id,
                "title": k.title,
                "researched_on": k.researched_on,
                "tags": k.tags,
                "status": k.status.as_str(),
                "stale": stale,
            })
        })
        .filter(|v| !stale_only || v["stale"] == json!(true))
        .collect();
    Envelope::ok(
        format!("{} knowledge entry(ies).", items.len()),
        json!({ "items": items }),
    )
}

/// knowledge.lookup。query が title / summary / tags に部分一致するエントリの要約を返す。
pub fn lookup_knowledge(owox_dir: &Path, query: &str) -> Envelope {
    let needle = query.trim().to_lowercase();
    if needle.is_empty() {
        return Envelope::failed("query が空");
    }
    let all = match list_knowledge(owox_dir) {
        Ok(a) => a,
        Err(err) => return Envelope::failed(err),
    };
    let matches: Vec<_> = all
        .iter()
        .filter(|k| {
            k.title.to_lowercase().contains(&needle)
                || k.summary.to_lowercase().contains(&needle)
                || k.tags.iter().any(|t| t.to_lowercase().contains(&needle))
        })
        .map(|k| {
            json!({
                "id": k.id,
                "title": k.title,
                "summary": k.summary,
                "status": k.status.as_str(),
            })
        })
        .collect();
    Envelope::ok(
        format!("{} match(es) for '{}'.", matches.len(), query.trim()),
        json!({ "matches": matches }),
    )
}

/// current かつ調査日から stale_days を超えているか。日付が読めなければ false。
fn is_stale(k: &Knowledge, today: &str, stale_days: u32) -> bool {
    if k.status != KnowledgeStatus::Current {
        return false;
    }
    match (ymd(today), ymd(&k.researched_on)) {
        (Some(t), Some(r)) => t - r > stale_days as i64,
        _ => false,
    }
}

/// `YYYYMMDD` を通日へ (decay.rs と同じ換算)。8 桁数字以外は None。
fn ymd(s: &str) -> Option<i64> {
    if s.len() != 8 || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let y: i64 = s[0..4].parse().ok()?;
    let m: i64 = s[4..6].parse().ok()?;
    let d: i64 = s[6..8].parse().ok()?;
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(era * 146_097 + doe - 719_468)
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
        let dir = std::env::temp_dir().join(format!("owox-knowledge-test-{pid}-{n}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn input(title: &str, summary: &str) -> KnowledgeInput {
        KnowledgeInput {
            title: title.to_string(),
            summary: summary.to_string(),
            sources: vec!["https://example.com/doc".to_string()],
            researched_on: None,
            tags: vec!["topic".to_string()],
            supersedes: Vec::new(),
        }
    }

    #[test]
    fn add_then_get_roundtrips() {
        let dir = tempdir();
        let env = add_knowledge(&dir, "20260616", input("HTTP caching", "Use ETag headers."));
        assert_eq!(env.status, Status::Ok);
        let id = env.data.unwrap()["id"].as_str().unwrap().to_string();

        let got = get_knowledge(&dir, &id);
        let data = got.data.unwrap();
        assert_eq!(data["title"], "HTTP caching");
        assert_eq!(data["researched_on"], "20260616");
        assert_eq!(data["summary"], "Use ETag headers.");
        assert_eq!(data["status"], "current");
        assert_eq!(data["sources"][0], "https://example.com/doc");
        assert_eq!(data["tags"][0], "topic");
    }

    #[test]
    fn researched_on_defaults_to_today() {
        let dir = tempdir();
        let env = add_knowledge(&dir, "20260616", input("x", "y"));
        let id = env.data.unwrap()["id"].as_str().unwrap().to_string();
        let k = load_knowledge(&dir, &id).unwrap();
        assert_eq!(k.researched_on, "20260616");
    }

    #[test]
    fn supersede_marks_old_superseded_and_links() {
        let dir = tempdir();
        let first = add_knowledge(&dir, "20260101", input("Auth flow", "OAuth v1 details."));
        let old_id = first.data.unwrap()["id"].as_str().unwrap().to_string();

        let mut newer = input("Auth flow", "OAuth v2 details.");
        newer.supersedes = vec![old_id.clone()];
        let env = add_knowledge(&dir, "20260616", newer);
        assert_eq!(env.status, Status::Ok);
        let new_id = env.data.unwrap()["id"].as_str().unwrap().to_string();

        // 旧は superseded、新は current で旧を指す。
        assert_eq!(
            load_knowledge(&dir, &old_id).unwrap().status,
            KnowledgeStatus::Superseded
        );
        let newk = load_knowledge(&dir, &new_id).unwrap();
        assert_eq!(newk.status, KnowledgeStatus::Current);
        assert_eq!(newk.supersedes, vec![old_id]);
    }

    #[test]
    fn secret_in_summary_fails_and_writes_nothing() {
        let dir = tempdir();
        let mut bad = input("leaky", "token: ghp_abcdefghijklmnopqrstuvwxyz0123456789");
        bad.sources = Vec::new();
        let env = add_knowledge(&dir, "20260616", bad);
        assert_eq!(env.status, Status::Failed);
        assert!(list_knowledge(&dir).unwrap().is_empty());
    }

    #[test]
    fn secret_in_source_fails() {
        let dir = tempdir();
        let mut bad = input("leaky", "clean summary");
        bad.sources = vec!["https://x/?token=ghp_abcdefghijklmnopqrstuvwxyz0123456789".to_string()];
        assert_eq!(add_knowledge(&dir, "20260616", bad).status, Status::Failed);
    }

    #[test]
    fn lookup_matches_title_summary_tags() {
        let dir = tempdir();
        add_knowledge(
            &dir,
            "20260616",
            input("Rate limiting", "Use a token bucket."),
        );
        let count = |q: &str| {
            lookup_knowledge(&dir, q).data.unwrap()["matches"]
                .as_array()
                .unwrap()
                .len()
        };
        assert_eq!(count("rate"), 1); // title
        assert_eq!(count("bucket"), 1); // summary
        assert_eq!(count("topic"), 1); // tag
        assert_eq!(count("nomatch"), 0);
    }

    #[test]
    fn list_filters_status_and_stale() {
        let dir = tempdir();
        add_knowledge(&dir, "20260101", input("old", "old summary"));
        add_knowledge(&dir, "20260616", input("fresh", "fresh summary"));

        // stale_days=90、today=20260616。20260101 は古い。
        let env = list_knowledge_envelope(&dir, None, false, "20260616", 90);
        let data = env.data.unwrap();
        let items = data["items"].as_array().unwrap();
        assert_eq!(items.len(), 2);
        let stale_count = items.iter().filter(|i| i["stale"] == json!(true)).count();
        assert_eq!(stale_count, 1);

        // stale_only。
        let env = list_knowledge_envelope(&dir, None, true, "20260616", 90);
        assert_eq!(env.data.unwrap()["items"].as_array().unwrap().len(), 1);

        // status filter (superseded は無し)。
        let env = list_knowledge_envelope(&dir, Some("superseded"), false, "20260616", 90);
        assert_eq!(env.data.unwrap()["items"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn superseded_never_stale() {
        let dir = tempdir();
        let first = add_knowledge(&dir, "20260101", input("a", "a"));
        let old_id = first.data.unwrap()["id"].as_str().unwrap().to_string();
        let mut newer = input("a", "b");
        newer.supersedes = vec![old_id.clone()];
        add_knowledge(&dir, "20260616", newer);

        let old = load_knowledge(&dir, &old_id).unwrap();
        assert!(!is_stale(&old, "20260616", 90));
    }
}
