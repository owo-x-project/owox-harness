//! Codex CLI 向け生成。第1対象。出力ラベルは英語固定 (owox-harness は i18n しない)。
//!
//! 生成するのは hook 登録 (hooks.json) と MCP サーバ登録 (config.toml の mcp_servers.owox) と skills。
//! AGENTS.md は廃止した: 床コンテキスト (向き付け・意図ルーティング・Vision・全体スタイル・状態) は
//! SessionStart hook が live 注入する (`docs/handoff/20260616-コンテキスト配信の再設計.md`)。
//! SessionStart は source=compact / resume でも再発火するので、圧縮・再開後も床が戻る
//! (PostCompact は context 注入不可・実機確認)。
//! 編集可能な複製を置かないことで AI が生成物を直接編集して canon を変える経路を断つ。
//!
//! 詳細の段階的開示 (常時読む量の削減) は owox の MCP tool と語トリガ注入で行う。
//! 役割の責務・権限は床の `## Orchestration` 節が能動提示し、spawn 時に親が組込み agent_type へ注入するフォールバック経路で届く。
//! 参照: docs/validation/20260620-Phase9-オーケストレーション実機.md

use crate::model::Canon;
use crate::skill::Skill;
use crate::target::{GeneratedFile, Target, Write};

/// Codex CLI 向け Target。
pub struct CodexTarget;

impl Target for CodexTarget {
    fn name(&self) -> &str {
        "codex"
    }

    fn generate(&self, _canon: &Canon) -> Vec<GeneratedFile> {
        // AGENTS.md は生成しない。床コンテキストは SessionStart / PostCompact hook が注入する。
        vec![
            GeneratedFile {
                path: ".codex/hooks.json".to_string(),
                contents: render_hooks_json(),
                executable: false,
                write: Write::Overwrite,
            },
            GeneratedFile {
                path: ".codex/config.toml".to_string(),
                contents: render_mcp_config(),
                executable: false,
                // config.toml は Codex の共有設定。owox のブロックだけマージし、
                // 人間が書いた他設定 (model 等) を壊さない。
                write: Write::MergeToml,
            },
        ]
    }

    /// Codex は repo 内 `.agents/skills/<id>/` を読む (一次情報。横断標準の配置)。
    ///
    /// SKILL.md は横断標準なので skill の文面を verbatim で出す。scripts は同梱する。
    /// Codex 固有メタ `agents/openai.yaml` は Codex の正式スキーマで常に明示する
    /// (`docs/decisions/20260613-Phase5-実機検証の是正.md`)。常に書くのは再生成 (昇格等) で
    /// 古い値が残らないようにするため。
    fn generate_skills(&self, skills: &[Skill]) -> Vec<GeneratedFile> {
        let mut files = Vec::new();
        for skill in skills {
            let base = format!(".agents/skills/{}", skill.id);
            files.push(GeneratedFile {
                path: format!("{base}/SKILL.md"),
                contents: skill.skill_md.clone(),
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
            files.push(GeneratedFile {
                path: format!("{base}/agents/openai.yaml"),
                contents: render_openai_yaml(skill),
                executable: false,
                write: Write::Overwrite,
            });
        }
        files
    }
}

/// skill の Codex 固有メタ `agents/openai.yaml` を Codex 正式スキーマで描画する。
///
/// `interface` は UI 表示と `$名前` 起動のため、`policy.allow_implicit_invocation` で
/// 自動起動の可否を出す。トップレベルに置くと Codex に無視され既定 true 扱いになるため
/// 必ず `policy` 配下へ入れる (`docs/decisions/20260613-Phase5-実機検証の是正.md`)。
/// 文字列は値のみ引用し (Codex の openai.yaml 規約)、キーは引用しない。
fn render_openai_yaml(skill: &Skill) -> String {
    format!(
        "interface:\n  display_name: \"{name}\"\n  short_description: \"{desc}\"\n  default_prompt: \"{prompt}\"\npolicy:\n  allow_implicit_invocation: {implicit}\n",
        name = yaml_escape(&skill.name),
        desc = yaml_escape(&skill.description),
        prompt = yaml_escape(&default_prompt(&skill.name, &skill.description)),
        implicit = skill.effective_implicit(),
    )
}

/// `$名前` 起動時の既定プロンプト。Codex 規約で `$名前` を必ず含める。
/// description を「Use $名前 to …」へ均す (先頭小文字化・末尾句点除去)。
fn default_prompt(name: &str, description: &str) -> String {
    let gist = description.trim().trim_end_matches('.');
    let mut chars = gist.chars();
    let gist = match chars.next() {
        Some(first) => format!("{}{}", first.to_lowercase(), chars.as_str()),
        None => return format!("Use ${name}."),
    };
    format!("Use ${name} to {gist}.")
}

/// YAML 二重引用符文字列向けの最小エスケープ。`\` と `"` を退避する。
fn yaml_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

/// MCP サーバ登録。owox serve を Codex の MCP server (stdio) として繋ぐ。
///
/// args にパスを焼かず serve のみ渡す。owox serve は起動時の作業ディレクトリから
/// `.owox` を上方探索するため、リポジトリを移しても動く (移植可能)。
fn render_mcp_config() -> String {
    // 薄いテンプレート。owox 管理ブロックだけを断片で持つ (write_all がマージ)。
    r#"[mcp_servers.owox]
command = "owox"
args = ["serve"]
"#
    .to_string()
}

/// hook 登録ファイル。インライン TOML はプロジェクトローカルで非発火報告 (Codex #17532)
/// があるため hooks.json 形式を採る。
///
/// スキーマは最上位 `hooks` ラッパ配下にイベント名 (キャメルケース) を置く
/// (Codex マニュアルの実例に一致)。このラッパが無いと Codex は hook を認識しない。
///
/// command は薄いシェルを介さず owox 実行ファイルを直接呼ぶ
/// (`docs/decisions/20260612-Phase3-hook実装.md`)。owox は PATH 上のシステムバイナリ。
/// 決定論ロジックは owox 側 (core) に集める。
///
/// - SessionStart (startup/resume/compact): 床コンテキストを additionalContext で注入。
///   matcher を startup だけでなく compact / resume へ広げ、圧縮・再開後も床を戻す
///   (PostCompact は additionalContext 非対応で context 注入できない・実機確認。
///   SessionStart は source=compact で再発火する。`docs/decisions/20260616-Phase7-コンテキスト配信の再設計.md`)
/// - PreToolUse (Bash/apply_patch/Edit/Write): Bash の不可逆操作を deny・git commit に完了確認、
///   編集対象の内容に出た用語の定義と change/safety policy を push
/// - UserPromptSubmit (matcher 非対応): プロンプトに出た用語の定義と rules/brand を能動 push
/// - Stop (matcher 無し): 完了前に verify・判断記録を促す
fn render_hooks_json() -> String {
    // 薄いテンプレート。手書き JSON で依存を増やさない。整形は固定。
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
        "matcher": "Bash|apply_patch|Edit|Write",
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

    fn skill(id: &str, implicit: bool, promoted: bool, scripts: Vec<ScriptFile>) -> Skill {
        Skill {
            id: id.to_string(),
            name: id.to_string(),
            description: "d".to_string(),
            skill_md: format!("---\nname: {id}\ndescription: d\n---\n\nbody\n"),
            implicit,
            promoted,
            human_gate: false,
            tests: Vec::new(),
            scripts,
        }
    }

    fn paths(files: &[GeneratedFile]) -> Vec<&str> {
        files.iter().map(|f| f.path.as_str()).collect()
    }

    #[test]
    fn skill_emits_skill_md_under_agents_skills() {
        let files = CodexTarget.generate_skills(&[skill("tidy", false, false, Vec::new())]);
        assert!(paths(&files).contains(&".agents/skills/tidy/SKILL.md"));
        let md = files.iter().find(|f| f.path.ends_with("SKILL.md")).unwrap();
        // SKILL.md は横断標準の文面を verbatim で出す。
        assert!(md.contents.contains("name: tidy"));
    }

    #[test]
    fn unpromoted_skill_disables_implicit() {
        // implicit 意図ありでも未昇格なら発火させない → openai.yaml で false。
        let files = CodexTarget.generate_skills(&[skill("grow", true, false, Vec::new())]);
        let yaml = files
            .iter()
            .find(|f| f.path == ".agents/skills/grow/agents/openai.yaml")
            .expect("openai.yaml が出る");
        assert!(yaml.contents.contains("allow_implicit_invocation: false"));
    }

    #[test]
    fn promoted_implicit_skill_enables_implicit() {
        // implicit かつ昇格 → openai.yaml で true (常に明示。再生成で古い値を残さない)。
        let files = CodexTarget.generate_skills(&[skill("trusted", true, true, Vec::new())]);
        let yaml = files
            .iter()
            .find(|f| f.path == ".agents/skills/trusted/agents/openai.yaml")
            .expect("openai.yaml が出る");
        assert!(yaml.contents.contains("allow_implicit_invocation: true"));
    }

    #[test]
    fn openai_yaml_uses_codex_schema() {
        // Codex 正式スキーマ: allow_implicit_invocation は policy 配下、interface を出す
        // (`docs/decisions/20260613-Phase5-実機検証の是正.md`)。
        let files = CodexTarget.generate_skills(&[skill("next", false, false, Vec::new())]);
        let yaml = &files
            .iter()
            .find(|f| f.path.ends_with("openai.yaml"))
            .unwrap()
            .contents;
        assert!(yaml.contains("policy:"), "policy ブロックを出す");
        assert!(
            yaml.contains("  allow_implicit_invocation: false"),
            "implicit は policy 配下へ入れ子"
        );
        assert!(yaml.contains("interface:"), "interface ブロックを出す");
        assert!(yaml.contains("display_name: \"next\""), "表示名を出す");
        // default_prompt は $名前 を含む (Codex 規約・$ 起動の発見性)。
        assert!(
            yaml.contains("default_prompt: \"Use $next "),
            "$名前 起動文"
        );
    }

    #[test]
    fn scripts_are_emitted_with_executable_bit() {
        let scripts = vec![ScriptFile {
            rel: "scripts/help.sh".to_string(),
            contents: "#!/bin/sh\n".to_string(),
            executable: true,
        }];
        let files = CodexTarget.generate_skills(&[skill("tidy", false, false, scripts)]);
        let script = files
            .iter()
            .find(|f| f.path == ".agents/skills/tidy/scripts/help.sh")
            .expect("script が出る");
        assert!(script.executable);
    }
}
