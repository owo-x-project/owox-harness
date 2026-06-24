//! ブランチ作業記憶層: git ブランチに紐づく作業メモ
//! (`docs/decisions/20260618-Phase9-ブランチ作業記憶層.md`)。
//!
//! work/ を拡張しブランチ別キーにする。git-ignored・床へ常時注入せずオンデマンド読み。
//! 並行作業 (worktree・マルチエージェント) でブランチ間の文脈が混ざらないよう、ブランチ名で分離する。
//! 自動削除しない: ブランチが消えたら腐敗検知 (stale-branch-memory) が剪定候補に出す。
//! 保管先 (work_root) は呼び出し側 (mcp) が worktree 横断で同じ場所へ正規化して渡す。

use std::path::{Path, PathBuf};

use serde_json::json;

use crate::envelope::Envelope;
use crate::secret;

/// 1 ブランチ分の作業記憶。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BranchMemory {
    /// ブランチ名 (ファイル先頭の見出しに保つ。ファイル名は別途サニタイズする)。
    pub branch: String,
    /// 作業メモ。各要素は `<日付>: <本文>` (task.note と同型の軽量記録)。
    pub notes: Vec<String>,
}

impl BranchMemory {
    /// 最新メモの日付 (YYYYMMDD)。腐敗検知の鮮度に使う。無ければ None。
    pub fn last_date(&self) -> Option<String> {
        self.notes
            .iter()
            .filter_map(|n| n.split_once(':').map(|(d, _)| d.trim().to_string()))
            .filter(|d| d.len() == 8 && d.chars().all(|c| c.is_ascii_digit()))
            .max()
    }
}

/// `<work_root>/branches/`。
fn branches_dir(work_root: &Path) -> PathBuf {
    work_root.join("branches")
}

/// ブランチ名をファイル名へサニタイズする。`/` は `__`、他の非英数は `_`。
/// 元のブランチ名は本文の見出しに保つ (ファイル名は照合用)。
fn sanitize(branch: &str) -> String {
    let mut out = String::new();
    for c in branch.chars() {
        if c == '/' {
            out.push_str("__");
        } else if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' {
            out.push(c);
        } else {
            out.push('_');
        }
    }
    out
}

fn branch_path(work_root: &Path, branch: &str) -> PathBuf {
    branches_dir(work_root).join(format!("{}.md", sanitize(branch)))
}

fn render(mem: &BranchMemory) -> String {
    let mut out = format!("# {}\n\n## Notes\n\n", mem.branch);
    for n in &mem.notes {
        out.push_str(&format!("- {n}\n"));
    }
    out
}

fn parse(text: &str) -> BranchMemory {
    let branch = text
        .lines()
        .map(str::trim)
        .find_map(|l| l.strip_prefix("# ").filter(|_| !l.starts_with("## ")))
        .unwrap_or("")
        .trim()
        .to_string();
    let mut notes = Vec::new();
    let mut in_notes = false;
    for line in text.lines() {
        let t = line.trim();
        if t == "## Notes" {
            in_notes = true;
            continue;
        }
        if t.starts_with("## ") {
            in_notes = false;
        }
        if in_notes && let Some(rest) = t.strip_prefix("- ") {
            notes.push(rest.trim().to_string());
        }
    }
    BranchMemory { branch, notes }
}

/// ブランチ作業記憶へメモを 1 件足す。書込時に秘密走査する (knowledge/experience と同じ守り)。
///
/// detached HEAD 等でブランチが取れない時は呼び出し側が "(detached)" 等の退避キーを渡す。
pub fn add_branch_note(work_root: &Path, branch: &str, today: &str, text: &str) -> Envelope {
    if branch.trim().is_empty() {
        return Envelope::failed("branch is empty");
    }
    if text.trim().is_empty() {
        return Envelope::failed("note text is empty");
    }
    let hits = secret::scan(text);
    if !hits.is_empty() {
        let ids: Vec<String> = hits.iter().map(|f| f.id.clone()).collect();
        return Envelope::failed(format!(
            "Secrets detected in the note; not recording. Remove them first. [{}]",
            ids.join(", ")
        ));
    }

    // 読込に失敗した時は上書きせず止める (既存メモの取りこぼし防止)。無い時は空から始まる。
    let mut mem = match load(work_root, branch) {
        Ok(m) => m,
        Err(err) => return Envelope::failed(err),
    };
    mem.branch = branch.to_string();
    mem.notes.push(format!("{today}: {}", text.trim()));

    let dir = branches_dir(work_root);
    if let Err(err) = std::fs::create_dir_all(&dir) {
        return Envelope::failed(format!("{} を作れない: {err}", dir.display()));
    }
    if let Err(err) = std::fs::write(branch_path(work_root, branch), render(&mem)) {
        return Envelope::failed(format!("ブランチ記憶を書けない: {err}"));
    }
    Envelope::ok(
        format!("Noted to branch memory for {branch}."),
        json!({ "branch": branch, "notes": mem.notes.len() }),
    )
}

/// 1 ブランチ分の記憶を読む。ファイルが無いのは「まだメモが無い」正常な空状態として
/// 空の記憶を返す。読込そのものに失敗した時だけ Err。
fn load(work_root: &Path, branch: &str) -> Result<BranchMemory, String> {
    let path = branch_path(work_root, branch);
    match std::fs::read_to_string(&path) {
        Ok(text) => Ok(parse(&text)),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(BranchMemory {
            branch: branch.to_string(),
            notes: Vec::new(),
        }),
        Err(err) => Err(format!("{} を読めない: {err}", path.display())),
    }
}

/// 全ブランチの記憶を読む。`<work_root>/branches/*.md`。ディレクトリが無ければ空。
pub fn list_branch_memories(work_root: &Path) -> Result<Vec<BranchMemory>, String> {
    let dir = branches_dir(work_root);
    let read = match std::fs::read_dir(&dir) {
        Ok(r) => r,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(format!("{} を読めない: {err}", dir.display())),
    };
    let mut mems = Vec::new();
    for entry in read {
        let path = entry.map_err(|e| e.to_string())?.path();
        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        let text = std::fs::read_to_string(&path)
            .map_err(|e| format!("{} を読めない: {e}", path.display()))?;
        mems.push(parse(&text));
    }
    mems.sort_by(|a, b| a.branch.cmp(&b.branch));
    Ok(mems)
}

/// 現在のブランチの記憶を全文返す (オンデマンド読み口・canon 直読み禁止と整合)。
pub fn get_branch_memory_envelope(work_root: &Path, branch: &str) -> Envelope {
    match load(work_root, branch) {
        Ok(mem) => {
            let reason = if mem.notes.is_empty() {
                format!("No notes recorded on branch {branch} yet.")
            } else {
                format!("Branch memory for {branch}.")
            };
            Envelope::ok(reason, json!({ "branch": mem.branch, "notes": mem.notes }))
        }
        Err(err) => Envelope::failed(err),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tempdir() -> PathBuf {
        use std::sync::atomic::{AtomicU32, Ordering};
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("owox-branchmem-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn add_then_roundtrip() {
        let root = tempdir();
        add_branch_note(&root, "feat/login", "20260618", "started the redirect");
        add_branch_note(&root, "feat/login", "20260618", "blocked on token shape");
        let mem = load(&root, "feat/login").unwrap();
        assert_eq!(mem.branch, "feat/login");
        assert_eq!(mem.notes.len(), 2);
        assert_eq!(mem.last_date().as_deref(), Some("20260618"));
    }

    #[test]
    fn branches_are_separated() {
        let root = tempdir();
        add_branch_note(&root, "feat/a", "20260618", "a note");
        add_branch_note(&root, "feat/b", "20260618", "b note");
        let all = list_branch_memories(&root).unwrap();
        assert_eq!(all.len(), 2);
        // 別ブランチの記憶は混ざらない。
        assert_eq!(load(&root, "feat/a").unwrap().notes.len(), 1);
    }

    #[test]
    fn slash_branches_do_not_collide() {
        let root = tempdir();
        add_branch_note(&root, "feat/a", "20260618", "slash");
        add_branch_note(&root, "feat-a", "20260618", "dash");
        assert_eq!(load(&root, "feat/a").unwrap().notes[0], "20260618: slash");
        assert_eq!(load(&root, "feat-a").unwrap().notes[0], "20260618: dash");
    }

    #[test]
    fn missing_branch_reads_as_empty_ok() {
        let root = tempdir();
        // メモを一度も残していないブランチは失敗ではなく空の記憶として返る。
        let mem = load(&root, "feat/never").unwrap();
        assert_eq!(mem.branch, "feat/never");
        assert!(mem.notes.is_empty());
        let env = get_branch_memory_envelope(&root, "feat/never");
        assert_eq!(env.status, crate::envelope::Status::Ok);
    }

    #[test]
    fn secret_is_rejected() {
        let root = tempdir();
        let env = add_branch_note(
            &root,
            "feat/x",
            "20260618",
            "token AKIAIOSFODNN7EXAMPLE in here",
        );
        assert_eq!(env.status, crate::envelope::Status::Failed);
    }
}
