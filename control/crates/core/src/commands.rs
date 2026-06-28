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

/// 曖昧な要望を質問で具体化する共通指針 (grill-me)。kickoff / req / design が共有する。
///
/// 一度に1問・コードで分かる事は自己解決・質問種別で形を変える (`docs/decisions/20260611-方向付け.md`,
/// `docs/decisions/20260627-判断2軸と対話kickoff.md`)。提示は `{{QUESTION_TOOL}}` プレースホルダで持ち、
/// target 生成時に各 CLI の質問ツール名へ写像する (`crate::target::apply_question_tool`)。判断型は
/// 推奨案を先頭の選択肢にする。正本は CLI 名を入れない (生成文はツール非依存)。
const GRILL_GUIDANCE: &str = "Grill one point at a time to make vague intent concrete; resolve what the code already shows yourself first (context scope codebase, or scope diff when a change is in flight) and only ask what the code cannot answer. Shape each point by its kind: for an elicit point ask an open question and paste no recommendation; for a confirm point state the decision owox already made and let the human only override it; for a judge point give the recommendation, the reason, and the options. Present each point to the human {{QUESTION_TOOL}}, and for a judge point make the recommended option the first choice. When an answer stays vague, ask one sharp follow-up before moving on.";

/// owox 標準コマンド (第1段階)。本文は owox 機能識別子だけを指し、CLI 名を入れない。
///
/// 本文は 1 つの owox tool を命令形で呼び、ファイル直読みや代替手順を禁じる
/// (散文の指示はモデルが別経路へ逸れる。`docs/decisions/20260613-Phase5-実機検証の是正.md`)。
fn standard_commands() -> Vec<Command> {
    // 質問で具体化する入口 (kickoff / req / design) は grill-me 指針を共有する。
    let mut commands = vec![
        Command {
            name: "kickoff".to_string(),
            description: "Start kickoff and surface the next setup point.".to_string(),
            body: format!(
                "Call kickoff, then call next. {GRILL_GUIDANCE} Hold product intent and safety for the human; decide and write action-axis setup yourself (project nature, indexes, setup files) without asking. Do the action owox proceeds with; do not ask the human about it. Do not read the canon files yourself."
            ),
        },
        Command {
            name: "req".to_string(),
            description: "Draft or refine requirements the way the project's nature calls for."
                .to_string(),
            body: format!(
                "Draft or refine requirements with the requirement tools. First call profile.get to see the active requirements-shape. {GRILL_GUIDANCE} If prfaq, think through the requirement as a short press release (who benefits and how) plus the key questions, and get human approval on what and why before building; if lightweight, capture a brief statement and acceptance criteria directly. Do not write the requirement until the intent is concrete. The requirement itself is the canonical record: distill what you drafted into requirement.create and requirement.add_criterion, each criterion with a verification link, rather than keeping a separate press-release document. Record the why and the benefit as a decision and link it. Tag each requirement's kind as functional or non-functional; keep technical and design constraints as decisions, not requirements. If prioritization is ideal-first, leave the priority ranking to a human. Do not read or edit requirement files under .owox/ directly."
            ),
        },
        Command {
            name: "design".to_string(),
            description: "Work out a design decision before implementing.".to_string(),
            body: format!(
                "Shape a design decision before writing code. First understand the structure (context scope codebase, and scope diff when a change is in flight). {GRILL_GUIDANCE} Design choices are tradeoffs: weigh options against responsibility boundaries, dependency direction, extensibility, and cleanliness, and prefer the future-proof and clean option even when it costs more now. Hold genuine product-intent or safety forks for the human; owox may settle reversible technical tradeoffs itself. Record the chosen design and why, plus the rejected alternatives and revisit conditions, with decision.record using kind \"design\", and link it to the requirement it serves. Enforceable structural constraints (dependency direction, layer boundaries) belong in quality.toml, not prose. Keep design and technical constraints as decisions, not requirements. Do not read the canon files yourself."
            ),
        },
    ];

    let defs: &[(&str, &str, &str)] = &[
        (
            "next",
            "See the intent gate and the action owox proceeds with.",
            "Call next. It splits into the human's intent gate (open decisions and prioritization the human decides) and the action owox proceeds with (shown last). Present the gate to the human; carry out the action owox proceeds with yourself without asking. If you need what changed, call context with scope diff. If you need a repo map before choosing files, call context with scope codebase. Do not read the canon files yourself.",
        ),
        (
            "status",
            "Summarize current state and blockers.",
            "Call next, then gate.list. Call verify.run too when the current check state matters. Do not read the canon files yourself.",
        ),
        (
            "decide",
            "Record or resolve a durable decision.",
            "Record a durable decision with decision.record. Use status open when it still needs human judgment. To resolve an open gate, call gate.approve yourself when the human decides to approve; its CLI confirmation prompt is the human's approval — do not ask the human to call it.",
        ),
        (
            "verify",
            "Check completion before finishing.",
            "Call verify.run and report work, requirement, and verification completion from that result. Do not run the project's checks yourself.",
        ),
        (
            "review",
            "Review a change with the right lenses.",
            "Call review.lenses, then verify.run, then context with scope diff. If structure is still unclear, call context with scope codebase. Review only what survives both confirmation and re-checking. Treat pruning as a proposal, not a blind delete.",
        ),
        (
            "task",
            "Manage work as verifiable tasks.",
            "Manage work with the task tools: list ready work with task.list, create new work with task.create, and finish work with task.close, which requires verification to pass. If the area to touch is unclear before starting, call context with scope codebase.",
        ),
        (
            "issues",
            "Sync tasks with GitHub issues two ways.",
            "Sync this project's tasks with GitHub issues, keeping owox as the source of truth. Use the gh command-line tool over Bash; owox stores only the mapping. First list tasks with task.list and read their external refs. To import: list issues with gh, and for each issue not already mapped to a task, create one with task.create and record the mapping by passing external \"github: owner/repo#<number>\". To publish: for each open task with no github external ref, create an issue with gh and record the returned number back onto the task with task.update external. Match an existing task to its issue by the github external ref, never by title, so re-syncing never creates duplicates. On any conflict the owox task wins; update the issue to match, not the task.",
        ),
        (
            "skill",
            "Grow and manage reusable skills.",
            "Call skill.list first. When a repeated local routine is deterministic, testable, and secret-free, route it through skill.register and skill.promote; treat it as a script-oriented skill. Record lessons with skill.remember.",
        ),
        (
            "memo",
            "Save a note to the right existing place by its content.",
            "Save the note where it belongs; do not invent a new place. Classify by content: a durable judgment that must not be silently reversed goes to decision.record; the current task's working state goes to task.note; state that only matters on this branch goes to branch.note; a fact learned from investigation goes to knowledge.add; a lesson about a skill goes to skill.remember; a message meant for the next whole session goes through handoff. If the right place is unclear, ask the human which one before saving.",
        ),
        (
            "handoff",
            "Summarize state for the next session.",
            "Produce a concise handoff for the following session. Call next for open gates, ready tasks, and stale items, verify.run for the current check state, context with scope diff for what changed, and branch.notes for branch-local notes. Summarize what changed, what is verified, the open decisions, ready tasks, branch notes, stale items, and the next step. Do not read the canon files yourself.",
        ),
    ];
    commands.extend(defs.iter().map(|(name, description, body)| Command {
        name: name.to_string(),
        description: description.to_string(),
        body: body.to_string(),
    }));
    commands
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
            "req", "memo", "design",
        ] {
            assert!(names.contains(&expected.to_string()), "missing {expected}");
        }
    }

    #[test]
    fn intent_eliciting_commands_carry_grill_guidance() {
        // 曖昧な要望を具体化する入口は grill-me 指針を持ち、質問提示プレースホルダ (target 生成で
        // 質問ツール名へ写像) と判断型の推奨先頭を指示する。正本に CLI 固有ツール名は入れない。
        let commands = standard_commands();
        for name in ["kickoff", "req", "design"] {
            let cmd = commands.iter().find(|c| c.name == name).unwrap();
            assert!(
                cmd.body.contains(crate::target::QUESTION_TOOL_PLACEHOLDER),
                "{name} が質問提示プレースホルダを欠く"
            );
            assert!(
                cmd.body.contains("recommended option the first choice"),
                "{name} が判断型の推奨先頭指示を欠く"
            );
            assert!(
                cmd.body.contains("one point at a time"),
                "{name} が一度に1問の指針を欠く"
            );
            assert!(
                !cmd.body.to_lowercase().contains("askuserquestion"),
                "{name} の正本に CLI 固有ツール名が混入"
            );
        }
    }

    #[test]
    fn grill_guidance_embeds_the_placeholder() {
        // 指針本体とプレースホルダ定数の同期を守る (片方だけ変えても気づけるように)。
        assert!(GRILL_GUIDANCE.contains(crate::target::QUESTION_TOOL_PLACEHOLDER));
    }

    #[test]
    fn memo_routes_to_existing_stores_without_new_store() {
        // 「メモして」の唯一の分類役。新ストアを作らず既存 5 保存先 + handoff へ振る。
        let memo = standard_commands()
            .into_iter()
            .find(|c| c.name == "memo")
            .expect("memo command");
        for store in [
            "decision.record",
            "task.note",
            "branch.note",
            "knowledge.add",
            "skill.remember",
        ] {
            assert!(memo.body.contains(store), "memo が {store} へ振らない");
        }
        // 分類不能時は人間へ確認する。
        assert!(memo.body.contains("ask the human"));
    }

    #[test]
    fn handoff_pulls_from_live_sources() {
        // 引き継ぎは会話ログでなく live ソースから組む (`docs/.../memo-伝言メモ不要.md`)。
        let handoff = standard_commands()
            .into_iter()
            .find(|c| c.name == "handoff")
            .expect("handoff command");
        for src in ["next", "verify.run", "scope diff", "branch.notes"] {
            assert!(handoff.body.contains(src), "handoff が {src} を使わない");
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
