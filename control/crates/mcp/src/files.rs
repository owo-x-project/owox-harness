//! 検証対象ファイルの列挙。quality 適応度関数のファイル走査に使う。
//!
//! `git ls-files` の tracked に未無視の未追跡を合わせて列挙し、build 生成物・無視ファイルを避け速い。
//! 新規未コミットのソースも品質バー走査の対象に含める。
//! git が無い・失敗時は単純な走査へ退避する (`.git` / `.owox` を除外)。
//! core は git/走査を持たず決定論なので、列挙は mcp 側で行い結果を渡す
//! (`docs/decisions/20260614-Phase6-quality適応度関数.md`)。

use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;

/// 変更状態。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeStatus {
    Added,
    Modified,
    Deleted,
    Renamed,
}

impl ChangeStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            ChangeStatus::Added => "added",
            ChangeStatus::Modified => "modified",
            ChangeStatus::Deleted => "deleted",
            ChangeStatus::Renamed => "renamed",
        }
    }
}

/// 変更ファイルの大まかな種別。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileKind {
    Canon,
    Docs,
    Source,
    Test,
    Config,
    Generated,
    Unknown,
}

impl FileKind {
    pub fn as_str(self) -> &'static str {
        match self {
            FileKind::Canon => "canon",
            FileKind::Docs => "docs",
            FileKind::Source => "source",
            FileKind::Test => "test",
            FileKind::Config => "config",
            FileKind::Generated => "generated",
            FileKind::Unknown => "unknown",
        }
    }

    pub fn is_canon_surface(self) -> bool {
        matches!(self, FileKind::Canon | FileKind::Docs)
    }
}

/// 変更ファイル 1 件。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangedFile {
    pub path: String,
    pub previous_path: Option<String>,
    pub status: ChangeStatus,
    pub kind: FileKind,
}

/// 差分基準。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffBase {
    pub name: String,
    pub rev: String,
}

/// work_dir 配下の検証対象ファイル (work_dir からの相対パス) を列挙する。
///
/// canon (`.owox/`) は正本であってコードではないので品質バー・重複検出の対象から外す。
/// 外さないと canon と、promote が生成する harness の写し (`.agents/` 等) が同一内容として
/// duplicate-file 誤検出され、verify.run / next を狼少年化させる。git 退避走査も同じ除外。
pub fn list_repo_files(work_dir: &Path) -> Vec<String> {
    if let Some(tracked) = git_ls_files(work_dir) {
        let mut set: std::collections::BTreeSet<String> = tracked.into_iter().collect();
        if let Some(untracked) = git_untracked(work_dir) {
            set.extend(untracked);
        }
        return set.into_iter().filter(|p| !is_canon(p)).collect();
    }
    let mut out = Vec::new();
    walk(work_dir, work_dir, &mut out);
    out
}

/// canon (`.owox/` 配下) か。コード走査の対象から外す。
fn is_canon(rel: &str) -> bool {
    rel == ".owox" || rel.starts_with(".owox/")
}

/// 今の変更で触れたファイル (work_dir からの相対パス) を集める。
///
/// HEAD との diff (staged + unstaged) と未追跡ファイルを合わせる。レビュー観点の機械選択に使う。
/// git が無い・失敗時は空 (普遍観点だけが選ばれ、漏れはしない側へ倒れる)。
pub fn changed_files(work_dir: &Path) -> Vec<String> {
    let mut set = std::collections::BTreeSet::new();
    // HEAD との差分 (staged + unstaged)。
    if let Some(lines) = git_lines(work_dir, &["diff", "--name-only", "-z", "HEAD"]) {
        set.extend(lines);
    }
    // 未追跡ファイル。
    if let Some(lines) = git_untracked(work_dir) {
        set.extend(lines);
    }
    set.into_iter().collect()
}

/// `main` 系統との共通祖先。見つからない時は `HEAD` へ退避する。
///
/// 優先順: 現在ブランチの上流追跡 (@{upstream}) → origin/main → main → HEAD。
/// DiffBase.name はどの基準を使ったかを示す。
pub fn main_merge_base(work_dir: &Path) -> DiffBase {
    // 上流追跡が設定されていればそれを最優先する。
    if let Some(upstream) = git_value(
        work_dir,
        &[
            "rev-parse",
            "--abbrev-ref",
            "--symbolic-full-name",
            "@{upstream}",
        ],
    ) && let Some(rev) = git_value(work_dir, &["merge-base", "HEAD", &upstream])
    {
        return DiffBase {
            name: format!("merge-base({upstream}, HEAD)"),
            rev,
        };
    }
    for candidate in ["origin/main", "main"] {
        if git_value(work_dir, &["rev-parse", "--verify", candidate]).is_none() {
            continue;
        }
        if let Some(rev) = git_value(work_dir, &["merge-base", "HEAD", candidate]) {
            return DiffBase {
                name: format!("merge-base({candidate}, HEAD)"),
                rev,
            };
        }
    }
    DiffBase {
        name: "HEAD".to_string(),
        rev: current_git_head(work_dir).unwrap_or_else(|| "HEAD".to_string()),
    }
}

/// 現在の `HEAD` の commit id。git が無い・失敗時は None。
pub fn current_git_head(work_dir: &Path) -> Option<String> {
    git_value(work_dir, &["rev-parse", "HEAD"])
}

/// 基準 commit から見た変更地図。未追跡も `added` として足す。
pub fn changed_files_since(work_dir: &Path, base_rev: &str) -> Vec<ChangedFile> {
    let mut by_path = BTreeMap::new();
    if let Some(entries) = git_name_status(
        work_dir,
        &["diff", "--name-status", "-z", "-M", base_rev, "--", "."],
    ) {
        for file in entries {
            by_path.insert(file.path.clone(), file);
        }
    }
    if let Some(untracked) = git_untracked(work_dir) {
        for path in untracked {
            by_path.entry(path.clone()).or_insert(ChangedFile {
                kind: classify_path(&path),
                path,
                previous_path: None,
                status: ChangeStatus::Added,
            });
        }
    }
    by_path.into_values().collect()
}

/// 作業ツリーの変更の署名。HEAD との diff 内容 + 未追跡一覧をハッシュする (.owox 除外)。
///
/// 同じ変更なら同じ署名。Stop の「前回促した時から変わったか」と verify.run の
/// 「この内容を検証済みか」の双方が同じ尺度で比べられるよう 1 箇所に集める。
/// git が無い・失敗時は None (呼び側が変更扱い・未検証扱いへ倒す。見逃さない・安全側)。
/// 未追跡ファイルは一覧のみで内容は見ない (作成時に一度促せば足りる)。
pub fn tree_signature(work_dir: &Path) -> Option<String> {
    use std::hash::{Hash, Hasher};
    let diff = Command::new("git")
        .arg("-C")
        .arg(work_dir)
        .args(["diff", "HEAD", "--", ".", ":(exclude).owox"])
        .output()
        .ok()?;
    if !diff.status.success() {
        return None;
    }
    let others = Command::new("git")
        .arg("-C")
        .arg(work_dir)
        .args([
            "ls-files",
            "-z",
            "--others",
            "--exclude-standard",
            "--",
            ".",
            ":(exclude).owox",
        ])
        .output()
        .ok()?;
    if !others.status.success() {
        return None;
    }
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    diff.stdout.hash(&mut hasher);
    others.stdout.hash(&mut hasher);
    Some(format!("{:016x}", hasher.finish()))
}

/// パスの大まかな種別。完全性より安定した機械分類を優先する。
pub fn classify_path(path: &str) -> FileKind {
    let lower = path.to_ascii_lowercase();
    if is_canon(path) || path == "AGENTS.md" || lower.starts_with(".codex/") || lower == ".codex" {
        return FileKind::Canon;
    }
    if lower.starts_with("docs/") || lower == "readme.md" || lower.ends_with("/readme.md") {
        return FileKind::Docs;
    }
    if lower.starts_with(".agents/")
        || lower == ".agents"
        || lower.starts_with("target/")
        || lower == "target"
        || lower.starts_with("dist/")
        || lower == "dist"
        || lower.starts_with("build/")
        || lower == "build"
        || lower.starts_with("coverage/")
        || lower == "coverage"
        || lower.starts_with("node_modules/")
        || lower == "node_modules"
        || lower.starts_with("vendor/")
        || lower == "vendor"
    {
        return FileKind::Generated;
    }
    if lower.contains("/tests/")
        || lower.starts_with("tests/")
        || lower.ends_with("_test.rs")
        || lower.ends_with("_spec.rs")
        || lower.ends_with(".test.ts")
        || lower.ends_with(".spec.ts")
        || lower.ends_with(".test.js")
        || lower.ends_with(".spec.js")
        || lower.ends_with(".snap")
    {
        return FileKind::Test;
    }
    if matches!(
        lower.as_str(),
        "cargo.toml"
            | "cargo.lock"
            | "package.json"
            | "package-lock.json"
            | "pnpm-lock.yaml"
            | "tsconfig.json"
            | "pyproject.toml"
            | "requirements.txt"
            | ".gitignore"
            | ".gitattributes"
            | ".editorconfig"
    ) || lower.ends_with(".toml")
        || lower.ends_with(".yaml")
        || lower.ends_with(".yml")
        || lower.ends_with(".json")
    {
        return FileKind::Config;
    }
    if lower.contains("/src/")
        || lower.starts_with("src/")
        || lower.starts_with("crates/")
        || lower.starts_with("bin/")
        || lower.starts_with("scripts/")
        || lower.ends_with(".rs")
        || lower.ends_with(".ts")
        || lower.ends_with(".tsx")
        || lower.ends_with(".js")
        || lower.ends_with(".jsx")
        || lower.ends_with(".py")
        || lower.ends_with(".go")
        || lower.ends_with(".java")
        || lower.ends_with(".sh")
    {
        return FileKind::Source;
    }
    FileKind::Unknown
}

/// git サブコマンドを `-z` 区切りで実行し行を集める。失敗なら None。
fn git_lines(work_dir: &Path, args: &[&str]) -> Option<Vec<String>> {
    let output = Command::new("git")
        .arg("-C")
        .arg(work_dir)
        .args(args)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    Some(
        output
            .stdout
            .split(|b| *b == 0)
            .filter(|s| !s.trim_ascii().is_empty())
            .map(|s| String::from_utf8_lossy(s).into_owned())
            .collect(),
    )
}

/// git の 1 行値。前後空白を落とす。失敗なら None。
fn git_value(work_dir: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(work_dir)
        .args(args)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if value.is_empty() { None } else { Some(value) }
}

/// `git diff --name-status -z` の結果を変更地図へ直す。失敗なら None。
fn git_name_status(work_dir: &Path, args: &[&str]) -> Option<Vec<ChangedFile>> {
    let output = Command::new("git")
        .arg("-C")
        .arg(work_dir)
        .args(args)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let parts: Vec<String> = output
        .stdout
        .split(|b| *b == 0)
        .filter(|s| !s.trim_ascii().is_empty())
        .map(|s| String::from_utf8_lossy(s).into_owned())
        .collect();
    Some(parse_name_status(&parts))
}

fn parse_name_status(parts: &[String]) -> Vec<ChangedFile> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < parts.len() {
        let status = parts[i].trim();
        i += 1;
        let Some(tag) = status.chars().next() else {
            continue;
        };
        match tag {
            'R' => {
                if i + 1 >= parts.len() {
                    break;
                }
                let from = parts[i].clone();
                let path = parts[i + 1].clone();
                i += 2;
                out.push(ChangedFile {
                    kind: classify_path(&path),
                    path,
                    previous_path: Some(from),
                    status: ChangeStatus::Renamed,
                });
            }
            'A' | 'M' | 'D' => {
                if i >= parts.len() {
                    break;
                }
                let path = parts[i].clone();
                i += 1;
                out.push(ChangedFile {
                    kind: classify_path(&path),
                    path,
                    previous_path: None,
                    status: match tag {
                        'A' => ChangeStatus::Added,
                        'D' => ChangeStatus::Deleted,
                        _ => ChangeStatus::Modified,
                    },
                });
            }
            _ => {
                if i >= parts.len() {
                    break;
                }
                let path = parts[i].clone();
                i += 1;
                out.push(ChangedFile {
                    kind: classify_path(&path),
                    path,
                    previous_path: None,
                    status: ChangeStatus::Modified,
                });
            }
        }
    }
    out
}

/// `git ls-files` で tracked ファイルを列挙する。git が無い・失敗なら None。
fn git_ls_files(work_dir: &Path) -> Option<Vec<String>> {
    git_lines(work_dir, &["ls-files", "-z"])
}

/// `.gitignore` 済みを除いた未追跡ファイルを列挙する。git が無い・失敗なら None。
fn git_untracked(work_dir: &Path) -> Option<Vec<String>> {
    git_lines(
        work_dir,
        &["ls-files", "-z", "--others", "--exclude-standard"],
    )
}

/// git が使えない時の退避走査。`.git` / `.owox` を除外して相対パスを集める。
fn walk(root: &Path, dir: &Path, out: &mut Vec<String>) {
    let Ok(read) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in read.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name == ".git" || name == ".owox" {
            continue;
        }
        if path.is_dir() {
            walk(root, &path, out);
        } else if let Ok(rel) = path.strip_prefix(root) {
            out.push(rel.to_string_lossy().into_owned());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ChangeStatus, FileKind, classify_path, is_canon, parse_name_status};

    #[test]
    fn canon_paths_excluded_from_scan() {
        // .owox 配下は正本なのでコード走査から外す。
        assert!(is_canon(".owox"));
        assert!(is_canon(".owox/skills/polish-ui-copy/SKILL.md"));
        assert!(is_canon(".owox/decisions/20260620-x.md"));
        // 生成 harness の写しやソースは外さない (duplicate 検出の対象は片側だけになり誤検出が消える)。
        assert!(!is_canon(".agents/skills/polish-ui-copy/SKILL.md"));
        assert!(!is_canon("src/ui/widget.sh"));
        // .owox を名前の一部に含むだけのパスは除外しない。
        assert!(!is_canon("src/.owox-helper.sh"));
        assert!(!is_canon("my.owox/x"));
    }

    #[test]
    fn classify_path_uses_stable_buckets() {
        assert_eq!(classify_path(".owox/rules.md"), FileKind::Canon);
        assert_eq!(classify_path("AGENTS.md"), FileKind::Canon);
        assert_eq!(classify_path("docs/requirements/x.md"), FileKind::Docs);
        assert_eq!(
            classify_path(".agents/skills/x/SKILL.md"),
            FileKind::Generated
        );
        assert_eq!(classify_path("crates/core/src/lib.rs"), FileKind::Source);
        assert_eq!(classify_path("crates/core/tests/api.rs"), FileKind::Test);
        assert_eq!(classify_path("Cargo.toml"), FileKind::Config);
    }

    #[test]
    fn parse_name_status_handles_rename_and_delete() {
        let parts = vec![
            "R100".to_string(),
            "old/path.rs".to_string(),
            "new/path.rs".to_string(),
            "D".to_string(),
            "docs/stale.md".to_string(),
        ];
        let parsed = parse_name_status(&parts);
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].status, ChangeStatus::Renamed);
        assert_eq!(parsed[0].previous_path.as_deref(), Some("old/path.rs"));
        assert_eq!(parsed[0].path, "new/path.rs");
        assert_eq!(parsed[1].status, ChangeStatus::Deleted);
        assert_eq!(parsed[1].kind, FileKind::Docs);
    }

    #[test]
    fn is_canon_surface_true_for_canon_and_docs() {
        // FileKind::is_canon_surface は Canon と Docs だけ true。
        assert!(FileKind::Canon.is_canon_surface());
        assert!(FileKind::Docs.is_canon_surface());
        assert!(!FileKind::Source.is_canon_surface());
        assert!(!FileKind::Test.is_canon_surface());
        assert!(!FileKind::Config.is_canon_surface());
        assert!(!FileKind::Generated.is_canon_surface());
        assert!(!FileKind::Unknown.is_canon_surface());
    }

    #[test]
    fn main_merge_base_falls_back_to_head_in_non_git_dir() {
        // git リポジトリでない一時ディレクトリでは HEAD フォールバックになる。
        let tmp = std::env::temp_dir().join(format!("owox-files-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        let base = super::main_merge_base(&tmp);
        // non-git なら name="HEAD", rev="HEAD"。
        assert_eq!(base.name, "HEAD");
        assert_eq!(base.rev, "HEAD");
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
