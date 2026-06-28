//! 使用履歴 (usage trail): `.owox/usage.log` に追記専用 (`docs/decisions/20260616-Phase8-パターンからスキル育成.md`)。
//!
//! 1 行 = `<YYYYMMDD> <name>`。name は tool 名 / コマンド名のみ (引数は残さない = 秘密漏れ・肥大の回避)。
//! 床コンテキストへ注入しない。頻出手順の検知 (`routine.rs`) の時だけ読む。
//! 追記は best-effort (失敗で作業を止めない)。肥大を避け直近のみ保つ (上限巻き取り)。

use std::io::Write;
use std::path::{Path, PathBuf};

/// 直近で保持する最大行数 (上限巻き取り)。超えたら先頭から捨てる。
const MAX_LINES: usize = 2000;

/// 巻き取りを試みるファイルサイズ閾値 (byte)。これを超えた時だけ末尾保持へ書き直す。
const TRIM_BYTES: u64 = 256 * 1024;

/// `.owox/usage.log`。
fn usage_path(owox_dir: &Path) -> PathBuf {
    owox_dir.join("usage.log")
}

/// 使用 1 件を追記する。best-effort (失敗で作業を止めない)。
///
/// name は単一トークン化する (空白を `_`)。`<YYYYMMDD> <name>` の形を崩さない。
pub fn record(owox_dir: &Path, today: &str, name: &str) {
    let name = sanitize(name);
    if name.is_empty() {
        return;
    }
    let path = usage_path(owox_dir);
    // 初回作成時に gitignore へ登録する (常時追記の churn を履歴へ乗せない)。
    if !path.exists() {
        ensure_ignored(owox_dir);
    }
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        let _ = writeln!(f, "{today} {name}");
    }
    // 肥大時のみ末尾 MAX_LINES へ巻き取る (毎回は読み直さない)。
    if let Ok(meta) = std::fs::metadata(&path)
        && meta.len() > TRIM_BYTES
    {
        trim_tail(&path);
    }
}

/// シェルコマンドを安全な分類名へ畳んで記録する。
///
/// 生コマンドや引数は残さず、script-skill 判定に要る最小の語彙だけを残す。
/// 未分類のものは広い `Bash` へ倒す。
pub fn record_shell(owox_dir: &Path, today: &str, command: &str) {
    record(owox_dir, today, &classify_shell(command));
}

/// 記録された name を順序つきで読む。読めなければ空 (検知を妨げない)。
pub fn read_names(owox_dir: &Path) -> Vec<String> {
    let Ok(text) = std::fs::read_to_string(usage_path(owox_dir)) else {
        return Vec::new();
    };
    text.lines()
        .filter_map(|l| l.split_whitespace().nth(1).map(str::to_string))
        .collect()
}

/// name を単一トークンへ整える (空白を `_`・前後トリム)。改行や空白で行形式が崩れないように。
fn sanitize(name: &str) -> String {
    name.split_whitespace().collect::<Vec<_>>().join("_")
}

/// シェルコマンドを安全な分類名へ畳む。
///
/// `rtk` は透過扱いにし、その後ろの本体だけを見る。
///
/// 保存カテゴリ (引数・パス・検索語は残さない原則は厳守):
///
/// - repo local・決定論寄り語彙: `Bash:rg` / `Bash:sed` / `Bash:awk` / `Bash:jq` / `Bash:yq` /
///   `Bash:cargo-test` / `Bash:npm-test` / `Bash:pytest` / `Bash:git-diff`
/// - 破壊的操作 (rm/delete 系): `Bash:rm`
/// - 外部サービス操作: `Bash:curl` / `Bash:gh` / `Bash:wget`
/// - ネットワーク/認証系: `Bash:ssh` / `Bash:scp` / `Bash:rsync`
/// - それ以外: `Bash`
///
/// 破壊的/外部サービス/ネットワーク系のカテゴリは script-skill 判定で除外シグナルとなる。
fn classify_shell(command: &str) -> String {
    let tokens: Vec<&str> = command.split_whitespace().collect();
    if tokens.is_empty() {
        return "Bash".to_string();
    }
    let body = if tokens.first() == Some(&"rtk") {
        &tokens[1..]
    } else {
        &tokens[..]
    };
    let Some(first) = body.first() else {
        return "Bash".to_string();
    };
    match *first {
        // repo local・決定論寄り語彙。
        "rg" => "Bash:rg".to_string(),
        "sed" => "Bash:sed".to_string(),
        "awk" => "Bash:awk".to_string(),
        "jq" => "Bash:jq".to_string(),
        "yq" => "Bash:yq".to_string(),
        "pytest" => "Bash:pytest".to_string(),
        "cargo" if body.get(1) == Some(&"test") => "Bash:cargo-test".to_string(),
        "npm" if body.get(1) == Some(&"test") => "Bash:npm-test".to_string(),
        "git" if body.get(1) == Some(&"diff") => "Bash:git-diff".to_string(),
        // 破壊的操作 (rm / delete 系)。script-skill 除外シグナル。
        "rm" => "Bash:rm".to_string(),
        // 外部サービス操作。script-skill 除外シグナル (秘密値・認証も含む)。
        "curl" => "Bash:curl".to_string(),
        "wget" => "Bash:wget".to_string(),
        "gh" => "Bash:gh".to_string(),
        // ネットワーク/認証系。script-skill 除外シグナル。
        "ssh" => "Bash:ssh".to_string(),
        "scp" => "Bash:scp".to_string(),
        "rsync" => "Bash:rsync".to_string(),
        _ => "Bash".to_string(),
    }
}

/// `.owox/.gitignore` に `usage.log` を冪等に足す。telemetry を履歴へ乗せない。
fn ensure_ignored(owox_dir: &Path) {
    let gitignore = owox_dir.join(".gitignore");
    let current = std::fs::read_to_string(&gitignore).unwrap_or_default();
    if current.lines().any(|l| l.trim() == "usage.log") {
        return;
    }
    let mut next = current;
    if !next.is_empty() && !next.ends_with('\n') {
        next.push('\n');
    }
    next.push_str("usage.log\n");
    let _ = std::fs::write(&gitignore, next);
}

/// ファイルを末尾 MAX_LINES 行へ巻き取る。best-effort。
fn trim_tail(path: &Path) {
    let Ok(text) = std::fs::read_to_string(path) else {
        return;
    };
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() <= MAX_LINES {
        return;
    }
    let tail = lines[lines.len() - MAX_LINES..].join("\n");
    let _ = std::fs::write(path, format!("{tail}\n"));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tempdir() -> PathBuf {
        use std::sync::atomic::{AtomicU32, Ordering};
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let pid = std::process::id();
        let dir = std::env::temp_dir().join(format!("owox-usage-test-{pid}-{n}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn record_appends_name_only() {
        let dir = tempdir();
        record(&dir, "20260616", "knowledge.add");
        record(&dir, "20260616", "task.create");
        assert_eq!(read_names(&dir), vec!["knowledge.add", "task.create"]);
    }

    #[test]
    fn sanitize_drops_args_whitespace() {
        // 引数つきで来ても name 部分だけ単一トークン化される (空白は潰す)。
        let dir = tempdir();
        record(&dir, "20260616", "git commit -m x");
        // 1 行目の 2 トークン目 = 整形後の name。
        assert_eq!(read_names(&dir), vec!["git_commit_-m_x"]);
    }

    #[test]
    fn record_shell_keeps_only_safe_vocab() {
        let dir = tempdir();
        record_shell(&dir, "20260626", "rtk rg -n mission crates/mcp/src");
        record_shell(&dir, "20260626", "cargo test -q");
        record_shell(&dir, "20260626", "git status --short");
        assert_eq!(read_names(&dir), vec!["Bash:rg", "Bash:cargo-test", "Bash"]);
    }

    #[test]
    fn empty_name_is_skipped() {
        let dir = tempdir();
        record(&dir, "20260616", "   ");
        assert!(read_names(&dir).is_empty());
    }

    #[test]
    fn read_missing_is_empty() {
        let dir = tempdir();
        assert!(read_names(&dir).is_empty());
    }

    /// 破壊的/外部サービス/ネットワーク系のカテゴリが正しく分類される。
    #[test]
    fn destructive_and_external_categories_classified() {
        let dir = tempdir();
        // 破壊的操作。
        record_shell(&dir, "20260627", "rm -rf /tmp/foo");
        // 外部サービス操作。
        record_shell(&dir, "20260627", "curl https://example.com");
        record_shell(&dir, "20260627", "wget https://example.com");
        record_shell(&dir, "20260627", "gh pr create");
        // ネットワーク/認証系。
        record_shell(&dir, "20260627", "ssh user@host");
        record_shell(&dir, "20260627", "scp foo user@host:/tmp");
        record_shell(&dir, "20260627", "rsync -av src/ dst/");
        assert_eq!(
            read_names(&dir),
            vec![
                "Bash:rm",
                "Bash:curl",
                "Bash:wget",
                "Bash:gh",
                "Bash:ssh",
                "Bash:scp",
                "Bash:rsync",
            ]
        );
    }
}
