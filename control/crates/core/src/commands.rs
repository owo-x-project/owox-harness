//! 入口 (slash 相当)。人間が叩く owox フローの入口を薄い skill として出す。
//!
//! Codex は repo 内 slash を廃止予定にし skills へ一本化したため、入口も skill で実現する
//! (`docs/decisions/20260613-Phase5-スキルと入口.md`)。各コマンドは
//! `.agents/skills/<name>/` の薄い skill になり、明示起動 (implicit=false) で
//! 対応する owox tool (next ・ context ・ verify.run 等) を命令形で呼ぶ
//! (`docs/decisions/20260613-Phase5-実機検証の是正.md`)。
//!
//! owox 標準コマンド (固定コア) を持ち、プロジェクトは `.owox/commands.toml` で追加できる。

use std::path::Path;

use crate::skill::Skill;

/// コマンド 1 件。生成時に薄い skill へ変換する。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Command {
    /// 名前 = 起動名・skill id。
    pub name: String,
    /// いつ使うか (skill の description)。
    pub description: String,
    /// 本文。対応する owox 機能を呼ぶトリガ。
    pub body: String,
}

impl Command {
    /// 薄い skill へ変換する。入口は明示起動のみ (implicit=false)・テスト不要。
    pub fn to_skill(&self) -> Skill {
        Skill {
            id: self.name.clone(),
            name: self.name.clone(),
            description: self.description.clone(),
            skill_md: render_skill_md(self),
            implicit: false,
            promoted: false,
            human_gate: false,
            tests: Vec::new(),
            scripts: Vec::new(),
        }
    }
}

/// コマンドの SKILL.md を組む。横断標準の frontmatter + 本文。
fn render_skill_md(cmd: &Command) -> String {
    format!(
        "---\nname: {}\ndescription: {}\n---\n\n{}\n",
        cmd.name, cmd.description, cmd.body
    )
}

/// 入口 skill 集合を返す。owox 標準 ∪ プロジェクト追加 (`.owox/commands.toml`)。
pub fn command_skills(owox_dir: &Path) -> Result<Vec<Skill>, String> {
    Ok(load_commands(owox_dir)?
        .iter()
        .map(Command::to_skill)
        .collect())
}

/// コマンド定義を読む。owox 標準に、プロジェクトの commands.toml を上書き・追加する。
pub fn load_commands(owox_dir: &Path) -> Result<Vec<Command>, String> {
    let mut commands = standard_commands();

    let path = owox_dir.join("commands.toml");
    match std::fs::read_to_string(&path) {
        Ok(text) => {
            for cmd in parse_commands_toml(&text)
                .map_err(|e| format!("{} を解釈できない: {e}", path.display()))?
            {
                upsert(&mut commands, cmd);
            }
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => return Err(format!("{} を読めない: {err}", path.display())),
    }

    Ok(commands)
}

/// 同名があれば差し替え、無ければ足す (プロジェクトが標準を上書き・拡張できる)。
fn upsert(commands: &mut Vec<Command>, cmd: Command) {
    if let Some(slot) = commands.iter_mut().find(|c| c.name == cmd.name) {
        *slot = cmd;
    } else {
        commands.push(cmd);
    }
}

/// commands.toml を読む。`[[command]]` の並び。未知キーは弾く。
/// aliases は受け付けるが現状の Codex skill では実現しないため取り込まない (前方互換)。
fn parse_commands_toml(text: &str) -> Result<Vec<Command>, String> {
    #[derive(serde::Deserialize)]
    struct Raw {
        #[serde(default)]
        command: Vec<CommandRaw>,
    }
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct CommandRaw {
        name: String,
        description: String,
        body: String,
        /// 別名。受け付けるが Codex skill では未実現 (将来・他 CLI 用)。
        #[serde(default)]
        #[allow(dead_code)]
        aliases: Vec<String>,
    }
    let raw: Raw = toml::from_str(text).map_err(|e| e.to_string())?;
    Ok(raw
        .command
        .into_iter()
        .map(|c| Command {
            name: c.name,
            description: c.description,
            body: c.body,
        })
        .collect())
}

/// owox 標準コマンド (第1段階)。本文は owox 機能識別子だけを指し、CLI 名を入れない。
///
/// 本文は 1 つの owox tool を命令形で呼び、ファイル直読みや代替手順を禁じる
/// (散文の指示はモデルが別経路へ逸れる。`docs/decisions/20260613-Phase5-実機検証の是正.md`)。
fn standard_commands() -> Vec<Command> {
    let defs: &[(&str, &str, &str)] = &[
        (
            "kickoff",
            "Orient at the start of a session before deciding what to do.",
            "Call the kickoff tool, then the next tool and the context tool. State the project's Vision, current phase, and nature, then propose the next concrete step. Before you start designing or implementing, check which of the project's practices and grown skills fit the task and use them. The kickoff tool bundles orientation: if no nature is set yet it returns a detected draft to propose to the human and set with profile.set after they confirm; for a brand-new project ask the human which named bundle fits and set it with profile.set. If you are adopting owox into an existing codebase, the kickoff tool also returns draft guardrails such as layers, boundaries, and irreversible operations — propose them to the human before adding any with canon.add. Do not read the canon files yourself; these tools return what you need.",
        ),
        (
            "req",
            "Draft or refine requirements the way the project's nature calls for.",
            "Draft or refine requirements with the requirement tools. First call profile.get to see the active requirements-shape: if prfaq, think through the requirement as a short press release (who benefits and how) plus the key questions, and get human approval on what and why before building; if lightweight, capture a brief statement and acceptance criteria directly. Either way the requirement itself is the canonical record: distill what you drafted into requirement.create and requirement.add_criterion, each criterion with a verification link, rather than keeping a separate press-release document. Record the why and the benefit as a decision and link it. Tag each requirement's kind as functional or non-functional; keep technical and design constraints as decisions, not requirements. If prioritization is ideal-first, leave the priority ranking to a human. Do not read or edit requirement files under .owox/ directly.",
        ),
        (
            "next",
            "See what to decide next and which tasks are ready.",
            "Call the next tool and report the open decisions to resolve and the tasks that are ready to start. Do not read the canon files yourself; the next tool returns this.",
        ),
        (
            "status",
            "Summarize where the project stands now.",
            "Summarize the current state. Call the next tool for open decisions and ready tasks, and list pending human gates with gate.list. Do not read the canon files yourself.",
        ),
        (
            "decide",
            "Record or resolve a durable decision.",
            "Record a durable decision with decision.record. Use status open when it still needs human judgment. To resolve an open gate, call gate.approve yourself when the human decides to approve; its CLI confirmation prompt is the human's approval — do not ask the human to call it.",
        ),
        (
            "verify",
            "Check completion before finishing work.",
            "Call verify.run and report completion in three kinds: work, requirement, and verification. Do not run the project's check scripts yourself; verify.run runs them and records the result.",
        ),
        (
            "review",
            "Review a change from several angles.",
            "Review the current change. First call review.lenses to get the perspectives that apply to it, and call verify.run to take in what machines already checked; do not re-judge what verification, the quality bars, or the requirement trace already report. Then review through each applicable perspective in turn: correctness, design, security, alignment with existing conventions and assets, requirements, and unnecessary leftovers to prune. For each finding, confirm it against the actual code, then try to refute it from another angle and keep only what survives. Treat pruning as a proposal: never delete blindly, route any removal through the deletion policy and re-run verify.run, and send irreversible deletions to a human.",
        ),
        (
            "task",
            "Manage work as verifiable tasks.",
            "Manage work with the task tools: list ready work with task.list, create new work with task.create, and finish work with task.close, which requires verification to pass.",
        ),
        (
            "issues",
            "Sync tasks with GitHub issues two ways.",
            "Sync this project's tasks with GitHub issues, keeping owox as the source of truth. Use the gh command-line tool over Bash; owox stores only the mapping. First list tasks with task.list and read their external refs. To import: list issues with gh, and for each issue not already mapped to a task, create one with task.create and record the mapping by passing external \"github: owner/repo#<number>\". To publish: for each open task with no github external ref, create an issue with gh and record the returned number back onto the task with task.update external. Match an existing task to its issue by the github external ref, never by title, so re-syncing never creates duplicates. On any conflict the owox task wins; update the issue to match, not the task.",
        ),
        (
            "skill",
            "Grow and manage reusable skills.",
            "Manage reusable skills with the skill tools: list them and their test and stage status with skill.list, register a draft with skill.register, promote a registered skill with skill.promote only after a human approves, and append a lesson with skill.remember. Author skill files under .owox/skills/. Do not create or register skills through other means.",
        ),
        (
            "handoff",
            "Summarize state for the next session.",
            "Produce a concise handoff: what changed, what is verified, the open decisions from the next tool, and the next step for the following session.",
        ),
    ];
    defs.iter()
        .map(|(name, description, body)| Command {
            name: name.to_string(),
            description: description.to_string(),
            body: body.to_string(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn tempdir() -> PathBuf {
        use std::sync::atomic::{AtomicU32, Ordering};
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let pid = std::process::id();
        let dir = std::env::temp_dir().join(format!("owox-cmd-test-{pid}-{n}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn standard_set_has_first_stage_commands() {
        let names: Vec<_> = standard_commands().into_iter().map(|c| c.name).collect();
        for expected in [
            "kickoff", "next", "status", "decide", "verify", "review", "task", "skill", "handoff",
            "req",
        ] {
            assert!(names.contains(&expected.to_string()), "missing {expected}");
        }
    }

    #[test]
    fn command_becomes_explicit_only_skill() {
        let cmd = &standard_commands()[0];
        let skill = cmd.to_skill();
        assert_eq!(skill.id, cmd.name);
        assert!(!skill.implicit); // 入口は明示起動のみ
        assert!(!skill.effective_implicit());
        assert!(skill.tests.is_empty()); // テスト不要
        assert!(skill.skill_md.contains(&format!("name: {}", cmd.name)));
    }

    #[test]
    fn standard_bodies_avoid_cli_names() {
        // 生成文はツール非依存。CLI 名を入れない。
        for cmd in standard_commands() {
            let lower = cmd.body.to_lowercase();
            assert!(!lower.contains("codex"), "{}: CLI 名が混入", cmd.name);
        }
    }

    #[test]
    fn project_commands_add_and_override() {
        let owox = tempdir();
        std::fs::write(
            owox.join("commands.toml"),
            "[[command]]\nname = \"deploy\"\ndescription = \"deploy it\"\nbody = \"Run the deploy.\"\n\n[[command]]\nname = \"next\"\ndescription = \"overridden\"\nbody = \"custom next\"\n",
        )
        .unwrap();
        let commands = load_commands(&owox).unwrap();
        // 追加された。
        assert!(commands.iter().any(|c| c.name == "deploy"));
        // 標準を上書きした。
        let next = commands.iter().find(|c| c.name == "next").unwrap();
        assert_eq!(next.description, "overridden");
    }

    #[test]
    fn aliases_are_accepted_but_ignored() {
        let owox = tempdir();
        std::fs::write(
            owox.join("commands.toml"),
            "[[command]]\nname = \"deploy\"\ndescription = \"d\"\nbody = \"b\"\naliases = [\"dp\"]\n",
        )
        .unwrap();
        // aliases キーを受けても壊れない (前方互換)。
        let commands = load_commands(&owox).unwrap();
        assert!(commands.iter().any(|c| c.name == "deploy"));
    }

    #[test]
    fn missing_commands_toml_yields_standard_only() {
        let owox = tempdir();
        assert_eq!(
            load_commands(&owox).unwrap().len(),
            standard_commands().len()
        );
    }
}
