//! 検証対象ファイルの列挙。quality 適応度関数のファイル走査に使う。
//!
//! `git ls-files` の tracked に未無視の未追跡を合わせて列挙し、build 生成物・無視ファイルを避け速い。
//! 新規未コミットのソースも品質バー走査の対象に含める。
//! git が無い・失敗時は単純な走査へ退避する (`.git` / `.owox` を除外)。
//! core は git/走査を持たず決定論なので、列挙は mcp 側で行い結果を渡す
//! (`docs/decisions/20260614-Phase6-quality適応度関数.md`)。

use std::path::Path;
use std::process::Command;

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
    use super::is_canon;

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
}
