# owox-harness

owox-harness turns AI-assisted development into a guided, verifiable workflow.

It gives AI agents the project rules, the next work to do, the checks that prove
progress, and the points where a human must decide. Instead of asking every
agent to guess the project context from a long prompt, owox-harness keeps the
context, rules, requirements, decisions, and lessons in a project canon that can
be generated into each supported AI tool.

The goal is simple: AI and humans should always know what to do next, why it
matters, and how to prove it is done.

Japanese README: `README.ja.md`

## Concept

owox-harness is for teams that want AI agents to do real work without turning
the project into a pile of chat history, stale prompts, and unclear decisions.

It is built around five outcomes:

- **The next step is always visible.** owox-harness can show open decisions,
  ready tasks, required context, and checks, so neither the human nor the agent
  has to rediscover the workflow each session.
- **Progress can be declared and verified.** Requirements, tasks, decisions, and
  checks are connected. Work is not just "done because the agent said so"; it
  can point to the condition that proves it.
- **Agents work with minimal context.** The agent receives the project facts it
  needs for the current task, not every document in the repository. Less noise
  means fewer missed rules and fewer invented assumptions.
- **The harness grows with the project.** As the project changes, owox-harness
  can record new rules, lessons, risks, skills, and checks, then use them in
  later sessions.
- **Experience can outlive one project.** Useful skills and lessons can be kept
  as reusable experience, not trapped inside one repository or one AI tool.

The core idea is not "AI does everything." The core idea is "people decide,
AI executes, owox-harness keeps the work clear, and the project remembers."

Supported targets today:

- Codex CLI
- Claude Code

## Install

Install the `owox` executable from GitHub Releases.

Linux and macOS:

```sh
curl -fsSL https://raw.githubusercontent.com/owoDra/workspace/main/control/scripts/setup.sh | sh
```

Windows PowerShell:

```powershell
irm https://raw.githubusercontent.com/owoDra/workspace/main/control/scripts/install.ps1 | iex
```

Then check the installed version:

```sh
owox --version
```

Default install locations:

- Linux and macOS: `~/.local/bin`
- Windows: `%LOCALAPPDATA%\owox\bin`

Set `OWOX_BIN_DIR` to choose another location.
Set `OWOX_VERSION` to install a fixed release, for example `owox-v0.1.0`.

## Basic Use

In a project that has an `.owox/` directory:

```sh
owox setup
```

This reads the project canon from `.owox/`, generates the agent setup files, and
checks that the result can be used.

For a project in another directory:

```sh
owox setup path/to/project
```

## Repository Layout

This product repository contains:

- `control/`: owox-harness source, docs, and tests
- `target/`: local sandbox for verification
- `.github/workflows/`: release workflow

The main owox-harness docs live under `control/docs/`.

## License

MIT License. See `LICENSE`.
