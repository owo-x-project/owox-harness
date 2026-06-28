//! `.owox/.cache/` の読み書き補助。session キャッシュと verify 署名の置き場。
//!
//! cache は git に乗せない (`.owox/.gitignore` で除外)。owox_dir 基準で扱い、
//! hook (cwd 経由) と serve (owox_dir 直) の双方から使えるようにする。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// session 任務種別。repo 共通でなく session ごとに持つ一時状態。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mission {
    Work,
    Kickoff,
    Review,
    Verify,
    Handoff,
}

impl Mission {
    pub fn parse(value: &str) -> Result<Mission, String> {
        match value.trim() {
            "work" => Ok(Mission::Work),
            "kickoff" => Ok(Mission::Kickoff),
            "review" => Ok(Mission::Review),
            "verify" => Ok(Mission::Verify),
            "handoff" => Ok(Mission::Handoff),
            other => Err(format!(
                "mission type は work / kickoff / review / verify / handoff のみ: {other}"
            )),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Mission::Work => "work",
            Mission::Kickoff => "kickoff",
            Mission::Review => "review",
            Mission::Verify => "verify",
            Mission::Handoff => "handoff",
        }
    }
}

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

/// 現在プロセスを起動した親進程番号。client ごとの橋渡しキーに使う。
pub fn launcher_pid() -> Option<u32> {
    #[cfg(unix)]
    {
        let pid = unsafe { libc::getppid() };
        if pid > 0 { Some(pid as u32) } else { None }
    }
    #[cfg(windows)]
    {
        let pid = std::process::id();
        let out = std::process::Command::new("powershell")
            .args([
                "-NoProfile",
                "-Command",
                &format!(
                    "(Get-CimInstance Win32_Process -Filter \"ProcessId={pid}\").ParentProcessId"
                ),
            ])
            .output()
            .ok()?;
        if !out.status.success() {
            return None;
        }
        String::from_utf8_lossy(&out.stdout).trim().parse().ok()
    }
    #[cfg(not(any(unix, windows)))]
    {
        None
    }
}

/// 親進程番号 -> client session_id の橋ファイル。hook が書き、serve が読む。
fn launcher_session_path(owox_dir: &Path, launcher_pid: u32) -> PathBuf {
    dir(owox_dir)
        .join("launcher-sessions")
        .join(format!("{launcher_pid}.json"))
}

/// hook から見えた client session_id を、同じ親進程配下の serve へ橋渡しする。
pub fn write_launcher_session(owox_dir: &Path, launcher_pid: u32, session_id: &str) {
    ensure_ignored(owox_dir);
    let path = launcher_session_path(owox_dir, launcher_pid);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string(session_id) {
        let _ = std::fs::write(&path, json);
    }
}

/// 同じ親進程の hook が記録した client session_id。無い・読めない時は None。
pub fn read_launcher_session(owox_dir: &Path, launcher_pid: u32) -> Option<String> {
    std::fs::read_to_string(launcher_session_path(owox_dir, launcher_pid))
        .ok()
        .and_then(|s| serde_json::from_str::<String>(&s).ok())
}

/// 現在の serve / hook が属する client session_id。橋が無い時は None。
pub fn current_session_id(owox_dir: &Path) -> Option<String> {
    launcher_pid().and_then(|pid| read_launcher_session(owox_dir, pid))
}

/// session 別任務 map。key=session_id, value=現在任務。
fn mission_path(owox_dir: &Path) -> PathBuf {
    dir(owox_dir).join("mission.json")
}

fn read_mission_map(owox_dir: &Path) -> BTreeMap<String, Mission> {
    std::fs::read_to_string(mission_path(owox_dir))
        .ok()
        .and_then(|s| serde_json::from_str::<BTreeMap<String, Mission>>(&s).ok())
        .unwrap_or_default()
}

fn write_mission_map(owox_dir: &Path, map: &BTreeMap<String, Mission>) -> Result<(), String> {
    ensure_ignored(owox_dir);
    let path = mission_path(owox_dir);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("{} を作れない: {e}", parent.display()))?;
    }
    let json = serde_json::to_string(map).map_err(|e| format!("mission を直列化できない: {e}"))?;
    std::fs::write(&path, json).map_err(|e| format!("{} へ書けない: {e}", path.display()))
}

/// 指定 session の現在任務。未設定なら既定 `work`。
pub fn mission_for_session(owox_dir: &Path, session_id: &str) -> Mission {
    read_mission_map(owox_dir)
        .get(session_id)
        .copied()
        .unwrap_or(Mission::Work)
}

/// 現在の client session の任務。橋が無い時は既定 `work`。
pub fn current_mission(owox_dir: &Path) -> Mission {
    current_session_id(owox_dir)
        .as_deref()
        .map(|sid| mission_for_session(owox_dir, sid))
        .unwrap_or(Mission::Work)
}

/// 現在 session の任務を設定する。client session_id が橋渡しされていない時は Err。
pub fn set_current_mission(owox_dir: &Path, mission: Mission) -> Result<(), String> {
    let session_id = current_session_id(owox_dir).ok_or_else(|| {
        "現在 session を特定できない。session-start hook 後に再試行すること。".to_string()
    })?;
    let mut map = read_mission_map(owox_dir);
    if mission == Mission::Work {
        map.remove(&session_id);
    } else {
        map.insert(session_id, mission);
    }
    write_mission_map(owox_dir, &map)
}

fn glossary_hits_path(owox_dir: &Path, session_id: &str) -> PathBuf {
    dir(owox_dir)
        .join("glossary-hits")
        .join(format!("{session_id}.json"))
}

fn read_glossary_hits_for_session(
    owox_dir: &Path,
    session_id: &str,
) -> Vec<owox_core::GlossaryTermHit> {
    std::fs::read_to_string(glossary_hits_path(owox_dir, session_id))
        .ok()
        .and_then(|s| serde_json::from_str::<Vec<owox_core::GlossaryTermHit>>(&s).ok())
        .unwrap_or_default()
}

pub fn read_current_glossary_hits(owox_dir: &Path) -> Vec<owox_core::GlossaryTermHit> {
    current_session_id(owox_dir)
        .as_deref()
        .map(|session_id| read_glossary_hits_for_session(owox_dir, session_id))
        .unwrap_or_default()
}

pub fn remember_glossary_hits(
    owox_dir: &Path,
    session_id: Option<&str>,
    hits: &[owox_core::GlossaryTermHit],
) {
    let Some(session_id) = session_id else {
        return;
    };
    if hits.is_empty() {
        return;
    }
    ensure_ignored(owox_dir);
    let path = glossary_hits_path(owox_dir, session_id);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let mut all = read_glossary_hits_for_session(owox_dir, session_id);
    all.extend(hits.iter().cloned());
    if all.len() > 512 {
        let keep_from = all.len() - 512;
        all = all.split_off(keep_from);
    }
    if let Ok(json) = serde_json::to_string(&all) {
        let _ = std::fs::write(&path, json);
    }
}

/// コードベース索引の area 1 件。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CodebaseArea {
    pub path: String,
    pub kind: String,
    pub role: String,
    #[serde(default)]
    pub evidence: Vec<String>,
}

/// コードベース索引の cache 本体。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CodebaseIndex {
    pub root_kind: String,
    #[serde(default)]
    pub package_files: Vec<String>,
    #[serde(default)]
    pub areas: Vec<CodebaseArea>,
    #[serde(default)]
    pub entrypoints: Vec<String>,
    #[serde(default)]
    pub checks: Vec<String>,
    #[serde(default)]
    pub generated_or_external: Vec<String>,
    #[serde(default)]
    pub source_files: Vec<String>,
    #[serde(default)]
    pub git_head: Option<String>,
    pub generated_on: String,
}

fn codebase_index_path(owox_dir: &Path) -> PathBuf {
    dir(owox_dir).join("codebase").join("index.json")
}

/// 保存済みコードベース索引。無い・読めない時は None。
pub fn read_codebase_index(owox_dir: &Path) -> Option<CodebaseIndex> {
    std::fs::read_to_string(codebase_index_path(owox_dir))
        .ok()
        .and_then(|s| serde_json::from_str::<CodebaseIndex>(&s).ok())
}

/// コードベース索引を書き出す。
pub fn write_codebase_index(owox_dir: &Path, index: &CodebaseIndex) -> Result<(), String> {
    ensure_ignored(owox_dir);
    let path = codebase_index_path(owox_dir);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("{} を作れない: {e}", parent.display()))?;
    }
    let json = serde_json::to_string(index)
        .map_err(|e| format!("codebase index を直列化できない: {e}"))?;
    std::fs::write(&path, json).map_err(|e| format!("{} へ書けない: {e}", path.display()))
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

    #[test]
    fn launcher_session_and_mission_are_keyed_per_session() {
        let owox = tempdir();
        write_launcher_session(&owox, 101, "claude-a");
        write_launcher_session(&owox, 202, "codex-b");
        assert_eq!(
            read_launcher_session(&owox, 101).as_deref(),
            Some("claude-a")
        );
        assert_eq!(
            read_launcher_session(&owox, 202).as_deref(),
            Some("codex-b")
        );

        let mut map = BTreeMap::new();
        map.insert("claude-a".to_string(), Mission::Kickoff);
        map.insert("codex-b".to_string(), Mission::Review);
        write_mission_map(&owox, &map).unwrap();

        assert_eq!(mission_for_session(&owox, "claude-a"), Mission::Kickoff);
        assert_eq!(mission_for_session(&owox, "codex-b"), Mission::Review);
        assert_eq!(mission_for_session(&owox, "missing"), Mission::Work);
    }

    #[test]
    fn codebase_index_round_trips() {
        let owox = tempdir();
        let index = CodebaseIndex {
            root_kind: "rust-workspace".to_string(),
            package_files: vec!["Cargo.toml".to_string()],
            areas: vec![CodebaseArea {
                path: "crates/mcp/src".to_string(),
                kind: "source".to_string(),
                role: "Rust source".to_string(),
                evidence: vec!["crates/mcp/src/main.rs".to_string()],
            }],
            entrypoints: vec!["crates/mcp/src/main.rs".to_string()],
            checks: vec!["cargo test".to_string()],
            generated_or_external: vec!["target/".to_string()],
            source_files: vec!["Cargo.toml".to_string()],
            git_head: Some("abc123".to_string()),
            generated_on: "20260626".to_string(),
        };
        write_codebase_index(&owox, &index).unwrap();
        assert_eq!(read_codebase_index(&owox), Some(index));
    }

    #[test]
    fn glossary_hits_are_session_scoped() {
        let owox = tempdir();
        remember_glossary_hits(
            &owox,
            Some("session-a"),
            &[owox_core::GlossaryTermHit {
                term: "TargetHarness".to_string(),
                source: "user prompt".to_string(),
                example: "user prompt".to_string(),
            }],
        );
        remember_glossary_hits(
            &owox,
            Some("session-b"),
            &[owox_core::GlossaryTermHit {
                term: "OtherTerm".to_string(),
                source: "lookup miss".to_string(),
                example: "OtherTerm".to_string(),
            }],
        );
        let a = read_glossary_hits_for_session(&owox, "session-a");
        let b = read_glossary_hits_for_session(&owox, "session-b");
        assert_eq!(a.len(), 1);
        assert_eq!(a[0].term, "TargetHarness");
        assert_eq!(b.len(), 1);
        assert_eq!(b[0].term, "OtherTerm");
    }
}
