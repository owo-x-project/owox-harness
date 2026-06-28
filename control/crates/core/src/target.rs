//! 生成対象 (AI CLI) の変換口。
//!
//! 各 CLI 向けの生成方法を [`Target`] で表し、登録表でまとめる。
//! CLI ごとの差分を変換側へ閉じ込め、core 内のべた分岐を防ぐ。

use std::path::{Path, PathBuf};

use crate::model::Canon;
use crate::skill::Skill;

/// 生成された 1 ファイル。出力先は target repo ルートからの相対パス。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedFile {
    /// target repo ルートからの相対パス。
    pub path: String,
    /// ファイル内容。Merge の時は owox 管理ブロックだけの断片。
    pub contents: String,
    /// 実行権限を付けるか。hook シェルなど。
    pub executable: bool,
    /// 書込み方法。owox が丸ごと持つ生成物は Overwrite、
    /// CLI と共有する設定ファイル (Codex の config.toml 等) は MergeToml。
    pub write: Write,
}

/// 生成物の書込み方法。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Write {
    /// 丸ごと上書きする。正本が単一の真実なので、owox が全体を持つファイル向け。
    #[default]
    Overwrite,
    /// 既存 TOML に owox 管理ブロックだけをマージする (他設定を壊さない)。
    /// contents は owox ブロックだけの TOML 断片。既存が無ければ新規作成する。
    MergeToml,
    /// 既存 JSON に owox 管理ブロックだけをマージする (他設定を壊さない)。
    /// Claude Code の `.claude/settings.json` (hooks) や `.mcp.json` (mcpServers) 向け。
    /// contents は owox ブロックだけの JSON 断片。既存が無ければ新規作成する。
    /// セマンティクスは MergeToml と同型 (オブジェクト同士は深く潜り、配列・スカラは断片で置換)。
    MergeJson,
}

/// skill 本文の質問提示プレースホルダ。生成時に各 CLI の質問ツール文言へ写像する。
/// CLI 非依存の正本 (commands.rs) はこの token を埋め込み、target 生成で具体化する。
pub const QUESTION_TOOL_PLACEHOLDER: &str = "{{QUESTION_TOOL}}";

/// 質問提示プレースホルダを CLI の質問ツール文言へ写像する。
///
/// `question_tool` が Some(名前) なら「using the <名前> tool」、None なら平文へ倒す
/// (その CLI に構造化質問ツールが無い)。token が無い skill 本文はそのまま返る。
pub fn apply_question_tool(skill_md: &str, question_tool: Option<&str>) -> String {
    let phrasing = match question_tool {
        Some(tool) => format!("using the {tool} tool"),
        None => {
            "by asking in plain text (this client has no interactive question tool)".to_string()
        }
    };
    skill_md.replace(QUESTION_TOOL_PLACEHOLDER, &phrasing)
}

/// AI CLI ごとの生成方法。
pub trait Target {
    /// 対象 CLI の識別子。
    fn name(&self) -> &str;
    /// 対話的質問ツールの名前 (人間へ質問を提示する構造化ツール)。無ければ None (平文で聞く)。
    /// 質問提示プレースホルダ ([`QUESTION_TOOL_PLACEHOLDER`]) の写像に使う。
    fn question_tool(&self) -> Option<&'static str> {
        None
    }
    /// 正本から生成物を出す (hook 登録 / 設定 / skills。ルート指示ファイルは廃止し床は hook 注入)。
    fn generate(&self, canon: &Canon) -> Vec<GeneratedFile>;
    /// 登録済みスキルを CLI が読む配置へ出す。
    ///
    /// SKILL.md 本体は横断標準なので skill が持つ文面を verbatim で出し (質問提示プレースホルダは
    /// この CLI の質問ツール文言へ写像する)、出力先と CLI 固有ファイル (自動起動可否のメタ等) だけ
    /// 各 Target が決める。テスト合格で絞った集合を呼び出し側が渡す (テスト実行=副作用は生成と分ける)。
    fn generate_skills(&self, skills: &[Skill]) -> Vec<GeneratedFile>;
}

/// 登録済みの全 Target を返す。CLI 追加はここへ 1 行加える。
pub fn registry() -> Vec<Box<dyn Target>> {
    vec![
        Box::new(crate::targets::codex::CodexTarget),
        Box::new(crate::targets::claude::ClaudeTarget),
    ]
}

/// 名前から Target を引く。
pub fn find(name: &str) -> Option<Box<dyn Target>> {
    registry().into_iter().find(|t| t.name() == name)
}

/// 生成物の書込失敗。書込先のパスを添えて原因を返す。
#[derive(Debug)]
pub struct WriteError {
    pub path: PathBuf,
    pub source: std::io::Error,
}

impl std::fmt::Display for WriteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} へ書けない: {}", self.path.display(), self.source)
    }
}

impl std::error::Error for WriteError {}

/// 生成物を `base` ルート下へ書く。親ディレクトリは作る。
///
/// Overwrite は丸ごと上書き (正本が単一の真実。再生成で同じ結果になる)。
/// MergeToml は既存 TOML に owox 管理ブロックだけ差し込む (人間の他設定を壊さない)。
pub fn write_all(base: &Path, files: &[GeneratedFile]) -> Result<(), WriteError> {
    for file in files {
        let dest = base.join(&file.path);
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent).map_err(|source| WriteError {
                path: parent.to_path_buf(),
                source,
            })?;
        }

        let contents = render_generated_file(&dest, file).map_err(|message| WriteError {
            path: dest.clone(),
            source: std::io::Error::new(std::io::ErrorKind::InvalidData, message),
        })?;

        std::fs::write(&dest, &contents).map_err(|source| WriteError {
            path: dest.clone(),
            source,
        })?;
        if file.executable {
            set_executable(&dest)?;
        }
    }
    Ok(())
}

/// 既存ファイルに対して generated file を適用したらどうなるかを返す。書き込みはしない。
pub fn render_generated_file(dest: &Path, file: &GeneratedFile) -> Result<String, String> {
    match file.write {
        Write::Overwrite => Ok(file.contents.clone()),
        Write::MergeToml => {
            let existing = std::fs::read_to_string(dest).unwrap_or_default();
            merge_toml(&existing, &file.contents)
        }
        Write::MergeJson => {
            let existing = std::fs::read_to_string(dest).unwrap_or_default();
            merge_json(&existing, &file.contents)
        }
    }
}

/// 既存 TOML へ owox 管理ブロック (断片) をマージする。
///
/// 断片のキーで既存を上書きするが、両方がテーブルなら再帰的に潜り、
/// 既存の他キー (人間が書いた model 設定や他の mcp_servers) は残す。
/// `toml` で読み書きするため、人間のコメント・並び順は保たれない (データは保つ)。
/// 出力は決定論的 (toml の既定はキー整列) なので、再生成で同じ結果になる。
fn merge_toml(existing: &str, fragment: &str) -> Result<String, String> {
    let mut base: toml::Table = if existing.trim().is_empty() {
        toml::Table::new()
    } else {
        toml::from_str(existing).map_err(|err| format!("既存 TOML を解釈できない: {err}"))?
    };
    let frag: toml::Table =
        toml::from_str(fragment).map_err(|err| format!("生成ブロックが不正: {err}"))?;

    merge_tables(&mut base, &frag);
    toml::to_string(&base).map_err(|err| format!("TOML を書けない: {err}"))
}

/// 断片テーブルを基底へ深くマージする。テーブル同士は再帰、それ以外は上書き。
fn merge_tables(base: &mut toml::Table, frag: &toml::Table) {
    for (key, value) in frag {
        match (base.get_mut(key), value) {
            (Some(toml::Value::Table(base_t)), toml::Value::Table(frag_t)) => {
                merge_tables(base_t, frag_t);
            }
            _ => {
                base.insert(key.clone(), value.clone());
            }
        }
    }
}

/// 既存 JSON へ owox 管理ブロック (断片) をマージする。
///
/// MergeToml と同型: オブジェクト同士は再帰的に潜り、配列・スカラは断片で置換する。
/// owox 断片はトップレベルで自分のキー (Claude Code なら hooks / mcpServers) だけを持つので、
/// 人間が書いた他キー (permissions・env・別 MCP サーバ) は残る。owox が管理する event の
/// hooks 配列だけは断片で置換される (再生成で重複しない冪等性を取るため・人間が同 event へ
/// 独自 hook を足すのは settings.local.json で住み分ける)。
/// 出力は整形済み・キー整列 (serde_json の Map は BTreeMap でないため挿入順) で決定論的。
fn merge_json(existing: &str, fragment: &str) -> Result<String, String> {
    let mut base: serde_json::Value = if existing.trim().is_empty() {
        serde_json::Value::Object(serde_json::Map::new())
    } else {
        serde_json::from_str(existing).map_err(|err| format!("既存 JSON を解釈できない: {err}"))?
    };
    let frag: serde_json::Value =
        serde_json::from_str(fragment).map_err(|err| format!("生成ブロックが不正: {err}"))?;

    merge_json_value(&mut base, &frag);
    serde_json::to_string_pretty(&base).map_err(|err| format!("JSON を書けない: {err}"))
}

/// 断片 JSON 値を基底へ深くマージする。オブジェクト同士は再帰、それ以外は上書き。
fn merge_json_value(base: &mut serde_json::Value, frag: &serde_json::Value) {
    match (base, frag) {
        (serde_json::Value::Object(base_o), serde_json::Value::Object(frag_o)) => {
            for (key, value) in frag_o {
                match base_o.get_mut(key) {
                    Some(base_v) => merge_json_value(base_v, value),
                    None => {
                        base_o.insert(key.clone(), value.clone());
                    }
                }
            }
        }
        (base, frag) => {
            *base = frag.clone();
        }
    }
}

#[cfg(unix)]
fn set_executable(path: &Path) -> Result<(), WriteError> {
    use std::os::unix::fs::PermissionsExt;
    let perm = std::fs::Permissions::from_mode(0o755);
    std::fs::set_permissions(path, perm).map_err(|source| WriteError {
        path: path.to_path_buf(),
        source,
    })
}

#[cfg(not(unix))]
fn set_executable(_path: &Path) -> Result<(), WriteError> {
    // 非 Unix では実行ビットの概念が無い。何もしない。
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apply_question_tool_maps_some_to_named_tool() {
        let md = format!("ask {QUESTION_TOOL_PLACEHOLDER} now");
        assert_eq!(
            apply_question_tool(&md, Some("AskUserQuestion")),
            "ask using the AskUserQuestion tool now"
        );
    }

    #[test]
    fn apply_question_tool_maps_none_to_plain_text() {
        let md = format!("ask {QUESTION_TOOL_PLACEHOLDER} now");
        let out = apply_question_tool(&md, None);
        assert!(out.contains("plain text"), "{out}");
        assert!(!out.contains("tool}}"), "プレースホルダが残る: {out}");
    }

    #[test]
    fn apply_question_tool_leaves_untokened_text_unchanged() {
        assert_eq!(
            apply_question_tool("no token here", Some("X")),
            "no token here"
        );
    }

    const OWOX_BLOCK: &str = "[mcp_servers.owox]\ncommand = \"owox\"\nargs = [\"serve\"]\n";

    #[test]
    fn merge_into_empty_creates_block() {
        let out = merge_toml("", OWOX_BLOCK).unwrap();
        assert!(out.contains("[mcp_servers.owox]"));
        assert!(out.contains("command = \"owox\""));
    }

    #[test]
    fn merge_preserves_human_settings_and_other_servers() {
        let existing = "model = \"gpt-5.4-mini\"\n\n[mcp_servers.other]\ncommand = \"foo\"\n";
        let out = merge_toml(existing, OWOX_BLOCK).unwrap();
        // 人間の設定と別サーバは残る。
        assert!(out.contains("model = \"gpt-5.4-mini\""));
        assert!(out.contains("[mcp_servers.other]"));
        assert!(out.contains("command = \"foo\""));
        // owox ブロックが足される。
        assert!(out.contains("[mcp_servers.owox]"));
    }

    #[test]
    fn merge_replaces_only_owox_block() {
        // 既存の owox ブロックが古い値でも、断片の値で置き換わる。
        let existing = "[mcp_servers.owox]\ncommand = \"old\"\nargs = [\"stale\"]\n";
        let out = merge_toml(existing, OWOX_BLOCK).unwrap();
        assert!(out.contains("command = \"owox\""));
        assert!(!out.contains("\"stale\""));
    }

    #[test]
    fn merge_is_idempotent() {
        let existing = "model = \"gpt-5.4-mini\"\n";
        let once = merge_toml(existing, OWOX_BLOCK).unwrap();
        let twice = merge_toml(&once, OWOX_BLOCK).unwrap();
        assert_eq!(once, twice, "再マージで同じ結果 (冪等)");
    }

    const OWOX_MCP: &str =
        "{\"mcpServers\":{\"owox\":{\"command\":\"owox\",\"args\":[\"serve\"]}}}";

    #[test]
    fn merge_json_into_empty_creates_block() {
        let out = merge_json("", OWOX_MCP).unwrap();
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["mcpServers"]["owox"]["command"], "owox");
    }

    #[test]
    fn merge_json_preserves_human_keys_and_other_servers() {
        // 人間の permissions と別 MCP サーバを壊さず owox ブロックを足す。
        let existing =
            r#"{"permissions":{"allow":["Bash"]},"mcpServers":{"other":{"command":"foo"}}}"#;
        let out = merge_json(existing, OWOX_MCP).unwrap();
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["permissions"]["allow"][0], "Bash");
        assert_eq!(v["mcpServers"]["other"]["command"], "foo");
        assert_eq!(v["mcpServers"]["owox"]["command"], "owox");
    }

    #[test]
    fn merge_json_replaces_owox_array_value() {
        // owox 管理ブロックの配列は断片で置換 (古い値を残さない)。
        let existing = r#"{"mcpServers":{"owox":{"command":"owox","args":["old"]}}}"#;
        let out = merge_json(existing, OWOX_MCP).unwrap();
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["mcpServers"]["owox"]["args"][0], "serve");
        assert_eq!(v["mcpServers"]["owox"]["args"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn merge_json_is_idempotent() {
        let existing = r#"{"permissions":{"allow":["Bash"]}}"#;
        let once = merge_json(existing, OWOX_MCP).unwrap();
        let twice = merge_json(&once, OWOX_MCP).unwrap();
        assert_eq!(once, twice, "再マージで同じ結果 (冪等)");
    }
}
