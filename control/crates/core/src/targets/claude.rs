//! Claude Code 向け生成。第2対象。出力ラベルは英語固定 (owox-harness は i18n しない)。
//!
//! 生成するのは hook 登録 (.claude/settings.json の hooks) と MCP サーバ登録 (.mcp.json の
//! mcpServers.owox) と skills (.claude/skills/<id>/)。
//! Claude Code の hook 入出力契約は Codex が踏襲した契約と同形 (snake_case 入力・
//! hookSpecificOutput / permissionDecision=deny / decision=block 出力) なので、`owox hook <event>`
//! バイナリをそのまま流用する。CLI 差は配置先と設定ファイル形式 (JSON) だけ。
//!
//! AGENTS.md / CLAUDE.md は生成しない: 床コンテキスト (向き付け・意図ルーティング・Vision・
//! 全体スタイル・状態) は SessionStart hook が live 注入する (Codex と同じ。
//! `docs/decisions/20260616-Phase7-コンテキスト配信の再設計.md`)。SessionStart は source=resume /
//! compact でも再発火するので圧縮・再開後も床が戻る。編集可能な複製を置かないことで AI が
//! 生成物を直接編集して canon を変える経路を断つ。
//!
//! settings.json と .mcp.json は人間と共有する設定なので MergeJson で owox 管理ブロックだけ差し込み、
//! 人間の他設定 (permissions・env・別 MCP サーバ) を壊さない。

use crate::model::Canon;
use crate::skill::Skill;
use crate::target::{GeneratedFile, Target, Write};

/// Claude Code 向け Target。
pub struct ClaudeTarget;

impl Target for ClaudeTarget {
    fn name(&self) -> &str {
        "claude"
    }

    /// Claude Code は構造化質問ツール AskUserQuestion を持つ (選択肢提示・推奨先頭に向く)。
    fn question_tool(&self) -> Option<&'static str> {
        Some("AskUserQuestion")
    }

    fn generate(&self, _canon: &Canon) -> Vec<GeneratedFile> {
        // ルート指示ファイルは生成しない。床コンテキストは SessionStart hook が注入する。
        vec![
            GeneratedFile {
                path: ".claude/settings.json".to_string(),
                contents: render_settings_json(),
                executable: false,
                // settings.json は Claude Code の共有設定。owox の hooks ブロックだけマージし、
                // 人間が書いた permissions / env / model を壊さない。
                write: Write::MergeJson,
            },
            GeneratedFile {
                path: ".mcp.json".to_string(),
                contents: render_mcp_json(),
                executable: false,
                // .mcp.json も共有設定。owox サーバだけマージし、人間の別 MCP サーバを残す。
                write: Write::MergeJson,
            },
        ]
    }

    /// Claude Code は repo 内 `.claude/skills/<id>/` を読む (一次情報。横断標準の配置)。
    ///
    /// SKILL.md は横断標準なので skill の文面を verbatim で出す (frontmatter の name / description が
    /// 発火条件)。scripts は同梱する。Codex の `agents/openai.yaml` のような implicit 明示制御は
    /// Claude Code の skill frontmatter に対応フィールドが無いため出さない (v1。
    /// `docs/decisions/20260621-Phase9-マルチCLI生成.md`)。テスト合格で絞った集合を呼び出し側が渡す。
    fn generate_skills(&self, skills: &[Skill]) -> Vec<GeneratedFile> {
        let mut files = Vec::new();
        for skill in skills {
            let base = format!(".claude/skills/{}", skill.id);
            files.push(GeneratedFile {
                path: format!("{base}/SKILL.md"),
                // 質問提示プレースホルダを Claude の質問ツール (AskUserQuestion) へ写像する。
                contents: crate::target::apply_question_tool(&skill.skill_md, self.question_tool()),
                executable: false,
                write: Write::Overwrite,
            });
            for script in &skill.scripts {
                files.push(GeneratedFile {
                    path: format!("{base}/{}", script.rel),
                    contents: script.contents.clone(),
                    executable: script.executable,
                    write: Write::Overwrite,
                });
            }
        }
        files
    }
}

/// MCP サーバ登録。owox serve を Claude Code の MCP server (stdio) として繋ぐ。
///
/// args にパスを焼かず serve のみ渡す。owox serve は起動時の作業ディレクトリから `.owox` を
/// 上方探索するため、リポジトリを移しても動く (移植可能)。owox ブロックだけの断片で持つ
/// (write_all が MergeJson でマージ)。
fn render_mcp_json() -> String {
    r#"{
  "mcpServers": {
    "owox": {
      "command": "owox",
      "args": ["serve"]
    }
  }
}
"#
    .to_string()
}

/// hook 登録。Claude Code は `.claude/settings.json` の `hooks` で event ごとに command を登録する。
///
/// command は薄いシェルを介さず owox 実行ファイルを直接呼ぶ
/// (`docs/decisions/20260612-Phase3-hook実装.md`)。owox は PATH 上のシステムバイナリ。
/// 決定論ロジックは owox 側 (core) に集める。
///
/// - SessionStart (startup/resume/compact): 床コンテキストを additionalContext で注入。
///   matcher を startup だけでなく compact / resume へ広げ、圧縮・再開後も床を戻す。
/// - PreToolUse (Bash/Edit/Write/MultiEdit): 不可逆操作を deny・git commit に完了確認、
///   編集対象の内容に出た用語の定義と change/safety policy を push。Claude Code の編集は専用 tool
///   (Edit/Write/MultiEdit) と Bash 経由なので両方を matcher に含める。NotebookEdit は notebook_path で
///   入力形が別のため v1 は対象外 (`docs/decisions/20260621-Phase9-マルチCLI生成.md`)。
/// - UserPromptSubmit: プロンプトに出た用語の定義と rules/brand を能動 push。
/// - Stop: 変更があれば検査を自動実行し、失敗なら block して修正へ向ける (通れば判断記録を促す)。
///
/// owox ブロックだけの断片で持つ (write_all が MergeJson でマージし人間の他設定を残す)。
fn render_settings_json() -> String {
    r#"{
  "hooks": {
    "SessionStart": [
      {
        "matcher": "startup|resume|compact",
        "hooks": [
          { "type": "command", "command": "owox hook session-start" }
        ]
      }
    ],
    "PreToolUse": [
      {
        "matcher": "Bash|Edit|Write|MultiEdit",
        "hooks": [
          { "type": "command", "command": "owox hook pre-tool-use" }
        ]
      }
    ],
    "UserPromptSubmit": [
      {
        "hooks": [
          { "type": "command", "command": "owox hook user-prompt-submit" }
        ]
      }
    ],
    "Stop": [
      {
        "hooks": [
          { "type": "command", "command": "owox hook stop" }
        ]
      }
    ]
  }
}
"#
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::skill::{ScriptFile, Skill};

    fn skill(id: &str, scripts: Vec<ScriptFile>) -> Skill {
        Skill {
            id: id.to_string(),
            name: id.to_string(),
            description: "d".to_string(),
            skill_md: format!("---\nname: {id}\ndescription: d\n---\n\nbody\n"),
            implicit: false,
            promoted: false,
            human_gate: false,
            tests: Vec::new(),
            scripts,
        }
    }

    fn paths(files: &[GeneratedFile]) -> Vec<&str> {
        files.iter().map(|f| f.path.as_str()).collect()
    }

    #[test]
    fn generate_emits_settings_and_mcp_as_merge_json() {
        let canon = Canon::default();
        let files = ClaudeTarget.generate(&canon);
        let settings = files
            .iter()
            .find(|f| f.path == ".claude/settings.json")
            .expect("settings.json を出す");
        assert_eq!(settings.write, Write::MergeJson);
        // hook command は owox バイナリを直接呼ぶ。
        assert!(settings.contents.contains("owox hook session-start"));
        assert!(settings.contents.contains("owox hook pre-tool-use"));
        let mcp = files
            .iter()
            .find(|f| f.path == ".mcp.json")
            .expect(".mcp.json を出す");
        assert_eq!(mcp.write, Write::MergeJson);
        assert!(mcp.contents.contains("\"owox\""));
        assert!(mcp.contents.contains("serve"));
    }

    #[test]
    fn settings_is_valid_json_with_four_events() {
        // 生成 settings.json は Claude Code が読める JSON。4 event を登録する。
        let json: serde_json::Value = serde_json::from_str(&render_settings_json()).unwrap();
        let hooks = json.get("hooks").expect("hooks ブロック");
        for event in ["SessionStart", "PreToolUse", "UserPromptSubmit", "Stop"] {
            assert!(hooks.get(event).is_some(), "{event} を登録");
        }
    }

    #[test]
    fn pre_tool_use_matcher_covers_edit_tools() {
        // Claude Code の編集 tool (Edit/Write/MultiEdit) と Bash を層ゲートへ載せる。
        // NotebookEdit は notebook_path で入力形が別のため v1 は対象外。
        let json: serde_json::Value = serde_json::from_str(&render_settings_json()).unwrap();
        let matcher = json["hooks"]["PreToolUse"][0]["matcher"].as_str().unwrap();
        for tool in ["Bash", "Edit", "Write", "MultiEdit"] {
            assert!(matcher.contains(tool), "{tool} を matcher に含む");
        }
    }

    #[test]
    fn mcp_json_is_valid_json() {
        let json: serde_json::Value = serde_json::from_str(&render_mcp_json()).unwrap();
        assert_eq!(json["mcpServers"]["owox"]["command"], "owox");
        assert_eq!(json["mcpServers"]["owox"]["args"][0], "serve");
    }

    #[test]
    fn skill_emits_skill_md_under_claude_skills() {
        let files = ClaudeTarget.generate_skills(&[skill("tidy", Vec::new())]);
        assert!(paths(&files).contains(&".claude/skills/tidy/SKILL.md"));
        let md = files.iter().find(|f| f.path.ends_with("SKILL.md")).unwrap();
        // SKILL.md は横断標準の文面を verbatim で出す。
        assert!(md.contents.contains("name: tidy"));
    }

    #[test]
    fn skill_maps_question_tool_placeholder_to_askuserquestion() {
        // 質問提示プレースホルダを持つ skill は Claude の AskUserQuestion へ写像される。
        let mut s = skill("kickoff", Vec::new());
        s.skill_md = format!(
            "---\nname: kickoff\ndescription: d\n---\n\nPresent each point {} now.\n",
            crate::target::QUESTION_TOOL_PLACEHOLDER
        );
        let files = ClaudeTarget.generate_skills(&[s]);
        let md = files.iter().find(|f| f.path.ends_with("SKILL.md")).unwrap();
        assert!(
            md.contents.contains("using the AskUserQuestion tool"),
            "{}",
            md.contents
        );
        assert!(
            !md.contents
                .contains(crate::target::QUESTION_TOOL_PLACEHOLDER)
        );
    }

    #[test]
    fn skill_emits_no_openai_yaml() {
        // Claude Code には Codex の openai.yaml 相当が無い。出さない。
        let files = ClaudeTarget.generate_skills(&[skill("grow", Vec::new())]);
        assert!(!paths(&files).iter().any(|p| p.ends_with("openai.yaml")));
    }

    #[test]
    fn scripts_are_emitted_with_executable_bit() {
        let scripts = vec![ScriptFile {
            rel: "scripts/help.sh".to_string(),
            contents: "#!/bin/sh\n".to_string(),
            executable: true,
        }];
        let files = ClaudeTarget.generate_skills(&[skill("tidy", scripts)]);
        let script = files
            .iter()
            .find(|f| f.path == ".claude/skills/tidy/scripts/help.sh")
            .expect("script が出る");
        assert!(script.executable);
    }
}
