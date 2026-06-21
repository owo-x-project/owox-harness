//! `.owox/.cache/` の読み書き補助。session キャッシュと verify 署名の置き場。
//!
//! cache は git に乗せない (`.owox/.gitignore` で除外)。owox_dir 基準で扱い、
//! hook (cwd 経由) と serve (owox_dir 直) の双方から使えるようにする。

use std::path::{Path, PathBuf};

/// `.owox/.cache/`。session キャッシュ・verify 署名を置く。
pub fn dir(owox_dir: &Path) -> PathBuf {
    owox_dir.join(".cache")
}

/// `.owox/.gitignore` に `.cache/` を冪等に足す。キャッシュを履歴へ乗せない。
pub fn ensure_ignored(owox_dir: &Path) {
    ensure_entry_ignored(owox_dir, ".cache/");
}

/// `.owox/.gitignore` に `entry` を冪等に足す。指定の作業域を履歴へ乗せない。
pub fn ensure_entry_ignored(owox_dir: &Path, entry: &str) {
    let bare = entry.trim_end_matches('/');
    let gitignore = owox_dir.join(".gitignore");
    let current = std::fs::read_to_string(&gitignore).unwrap_or_default();
    if current.lines().any(|l| {
        let t = l.trim();
        t == entry || t == bare
    }) {
        return;
    }
    let mut next = current;
    if !next.is_empty() && !next.ends_with('\n') {
        next.push('\n');
    }
    next.push_str(entry);
    next.push('\n');
    let _ = std::fs::write(&gitignore, next);
}

/// 最後に verify.run を走らせた時の記録 (作業ツリー署名 + 検査結果) のファイル。
///
/// session でなく repo 単位で持つ。署名は作業ツリーの内容ハッシュなので session を跨いでも
/// 「この内容を verify した」が正しく一致する。verify.run (serve) は session_id を持たない。
fn verify_signature_path(owox_dir: &Path) -> PathBuf {
    dir(owox_dir).join("last-verify.json")
}

/// 最後に verify.run を走らせた記録。署名と検査結果を保ち、commit ゲートが作業ツリー同一時に
/// 検査の二重実行を避けるために再利用する。
#[derive(serde::Serialize, serde::Deserialize)]
pub struct VerifyRecord {
    /// 走らせた時の作業ツリー署名。
    pub signature: String,
    /// 検査の総合判定 (`passed` / `failed` / `needs_human`。needs_human は検査未設定)。
    pub verification: String,
    /// 失敗した検査名 (verification=`failed` の時のみ)。
    #[serde(default)]
    pub failed: Vec<String>,
}

/// 最後に verify.run を走らせた記録。無い・読めない (旧形式含む) 時は None。
pub fn read_verify_record(owox_dir: &Path) -> Option<VerifyRecord> {
    std::fs::read_to_string(verify_signature_path(owox_dir))
        .ok()
        .and_then(|s| serde_json::from_str::<VerifyRecord>(&s).ok())
}

/// 最後に verify.run を走らせた時の作業ツリー署名。無い・読めない時は None。
/// Stop の「verify.run 済み・以降変更なし」判定に使う (合否は問わない)。
pub fn read_verify_signature(owox_dir: &Path) -> Option<String> {
    read_verify_record(owox_dir).map(|r| r.signature)
}

/// verify.run 実行時の記録 (署名 + 検査結果) を保存する。書けない時は何もしない (作業を妨げない)。
pub fn write_verify_record(owox_dir: &Path, record: &VerifyRecord) {
    ensure_ignored(owox_dir);
    let path = verify_signature_path(owox_dir);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string(record) {
        let _ = std::fs::write(&path, json);
    }
}

/// 自動承認の窓 (人間が「寝てる間進めて」と開けた状態) のファイル。
///
/// session でなく repo 単位で持つ。serve の tool は session_id を持たないため、
/// 「セッション限り」は session_start hook が毎回この窓を消すことで成り立つ
/// (`docs/decisions/20260619-承認と自動改善ループ.md`)。
fn auto_window_path(owox_dir: &Path) -> PathBuf {
    dir(owox_dir).join("auto-approve.json")
}

/// 自動承認の窓が開いているか。ファイルが在れば開いている。
pub fn auto_window_open(owox_dir: &Path) -> bool {
    auto_window_path(owox_dir).exists()
}

/// 自動承認の窓を開ける。人間が gate.auto_enable を承認した時だけ呼ぶ。
pub fn open_auto_window(owox_dir: &Path) {
    ensure_ignored(owox_dir);
    let path = auto_window_path(owox_dir);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(&path, "{}");
}

/// 自動承認の窓を閉じる。gate.auto_disable と、毎セッション開始時の session_start が呼ぶ。
pub fn close_auto_window(owox_dir: &Path) {
    let _ = std::fs::remove_file(auto_window_path(owox_dir));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tempdir() -> PathBuf {
        use std::sync::atomic::{AtomicU32, Ordering};
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let pid = std::process::id();
        let dir = std::env::temp_dir().join(format!("owox-cache-test-{pid}-{n}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn auto_window_opens_and_closes() {
        let owox = tempdir();
        assert!(!auto_window_open(&owox));
        open_auto_window(&owox);
        assert!(auto_window_open(&owox));
        // 冪等に再度開けても開いたまま。
        open_auto_window(&owox);
        assert!(auto_window_open(&owox));
        close_auto_window(&owox);
        assert!(!auto_window_open(&owox));
        // 閉じた状態で再度閉じても落ちない。
        close_auto_window(&owox);
        assert!(!auto_window_open(&owox));
    }
}
