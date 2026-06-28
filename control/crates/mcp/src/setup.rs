//! setup サブコマンド。別 target repo へ owox を導入する手順を自動化する。
//!
//! `owox setup [dir]` = 正本読込 → 生成 (canon 駆動・config.toml の targets を各々) → 検査 → 報告。
//! バイナリの install は範囲外 (配布は Phase9。`docs/decisions/20260613-Phase5-スキルと入口.md`)。
//! setup は owox 配置済みを前提に、設定生成と「正しく繋がるか」の検査に絞る。
//!
//! 決定論ロジックは core。ここは入出力の配線と検査の組み立てのみ (generate と同様)。

use std::collections::BTreeMap;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::ExitCode;

use console::{Key, Style, Term, style};
use dialoguer::{Confirm, Input, theme::ColorfulTheme};
use owox_core::{Brand, Canon, GeneratedFile, TargetSpec, Targets};

/// 検査 1 件の結果。setup の「正しく繋がるか」を人間へ示す。
struct Check {
    name: String,
    ok: bool,
    detail: String,
}

struct BootstrapAnswers {
    targets: Vec<String>,
    vision: String,
}

/// `owox setup [dir]` を捌く。
///
/// `dir` は target repo ルート (既定はカレント)。`dir/.owox/` を読み、
/// config.toml の targets を各々の out へ生成し、検査して報告する。
pub fn run(args: &[String]) -> ExitCode {
    let base: PathBuf = args
        .first()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));

    let owox_dir = base.join(".owox");
    if !owox_dir.exists()
        && let Err(err) = bootstrap_canon(&base, &owox_dir)
    {
        eprintln!("owox setup: {err}");
        return ExitCode::FAILURE;
    }
    let canon = match owox_core::load_canon(&owox_dir) {
        Ok(canon) => canon,
        Err(err) => {
            eprintln!("owox setup: could not read canon: {err}");
            return ExitCode::FAILURE;
        }
    };

    // 生成対象は canon 駆動。config.toml に targets が無ければ codex を既定にする
    // (第1対象。単一 CLI を最小設定で導入できる)。
    let targets: Vec<(String, String)> = if canon.targets.entries.is_empty() {
        eprintln!("owox setup: config.toml has no targets; using codex by default");
        vec![("codex".to_string(), ".".to_string())]
    } else {
        canon
            .targets
            .entries
            .iter()
            .map(|t| (t.name.clone(), t.out_dir.clone()))
            .collect()
    };

    // 登録済みスキル (テスト合格・適格) を算出する。テスト実行=副作用はここで起こす。
    let repo_root = base.as_path();
    let registered = match owox_core::registered_skills(&owox_dir, repo_root) {
        Ok(skills) => skills,
        Err(err) => {
            eprintln!("owox setup: could not read skills: {err}");
            return ExitCode::FAILURE;
        }
    };

    // 入口 (コマンド) を薄い skill として加える。owox 標準 ∪ プロジェクト追加。
    let commands = match owox_core::command_skills(&owox_dir) {
        Ok(skills) => skills,
        Err(err) => {
            eprintln!("owox setup: could not read commands: {err}");
            return ExitCode::FAILURE;
        }
    };

    // 登録済みスキルと入口 skill を一括で生成する (同じ `.agents/skills/` 配置)。
    let mut skills = registered;
    skills.extend(commands);

    let mut generated: Vec<(PathBuf, GeneratedFile)> = Vec::new();
    for (name, out) in &targets {
        let Some(target) = owox_core::find(name) else {
            eprintln!("owox setup: unknown target CLI: {name}");
            return ExitCode::from(2);
        };
        let out_root = base.join(out);
        let mut files = target.generate(&canon);
        files.extend(target.generate_skills(&skills));
        if let Err(err) = owox_core::write_all(&out_root, &files) {
            eprintln!("owox setup: {err}");
            return ExitCode::FAILURE;
        }
        for f in files {
            generated.push((out_root.join(&f.path), f));
        }
    }

    let checks = run_checks(&generated);
    report(&generated, &checks);

    // 検査に失敗があっても生成自体は済んでいる。導入の不備は警告として伝え、
    // 終了コードは検査結果で分ける (CI から繋ぎの妥当性を判定できる)。
    if checks.iter().all(|c| c.ok) {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// 導入が正しく繋がるかの検査。
///
/// - owox が PATH 上にあるか (MCP 登録の `command = "owox"` が解決できるか)
/// - 生成した設定ファイルが妥当か (.toml / .json が解釈できる。既存設定とのマージ崩れも捕る)
fn run_checks(generated: &[(PathBuf, GeneratedFile)]) -> Vec<Check> {
    let mut checks = vec![check_owox_on_path()];
    for (path, _) in generated {
        if let Some(check) = check_generated_file(path) {
            checks.push(check);
        }
    }
    checks
}

/// owox 実行ファイルが PATH 上で解決できるか。
///
/// 生成した MCP 登録は `command = "owox"` (パスを焼かない。移植可能)。
/// Codex がこれを起動できるよう、PATH 上に owox が要る。
fn check_owox_on_path() -> Check {
    let found = std::env::var_os("PATH")
        .map(|paths| {
            std::env::split_paths(&paths).any(|dir| {
                let candidate = dir.join("owox");
                candidate.is_file()
            })
        })
        .unwrap_or(false);

    if found {
        Check {
            name: "owox on PATH".to_string(),
            ok: true,
            detail: "ok".to_string(),
        }
    } else {
        Check {
            name: "owox on PATH".to_string(),
            ok: false,
            detail: "owox not found on PATH. Place the owox binary on PATH so the MCP server (command = \"owox\") can start.".to_string(),
        }
    }
}

/// 生成した設定ファイルの妥当性。拡張子で .toml / .json を解釈し、壊れていれば失敗を返す。
/// 対象外の拡張子は None (検査しない)。
fn check_generated_file(path: &Path) -> Option<Check> {
    let ext = path.extension().and_then(|e| e.to_str())?;
    let label = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("file")
        .to_string();
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(err) => {
            return Some(Check {
                name: format!("{label} readable"),
                ok: false,
                detail: format!("could not read: {err}"),
            });
        }
    };

    match ext {
        "toml" => Some(parse_check(
            &label,
            "toml",
            toml::from_str::<toml::Table>(&text)
                .map(|_| ())
                .map_err(|e| e.to_string()),
        )),
        "json" => Some(parse_check(
            &label,
            "json",
            serde_json::from_str::<serde_json::Value>(&text)
                .map(|_| ())
                .map_err(|e| e.to_string()),
        )),
        _ => None,
    }
}

/// 解釈結果を Check へ。
fn parse_check(label: &str, kind: &str, result: Result<(), String>) -> Check {
    match result {
        Ok(()) => Check {
            name: format!("{label} valid {kind}"),
            ok: true,
            detail: "ok".to_string(),
        },
        Err(err) => Check {
            name: format!("{label} valid {kind}"),
            ok: false,
            detail: err,
        },
    }
}

/// 生成物と検査結果を人間向けに報告する。
fn report(generated: &[(PathBuf, GeneratedFile)], checks: &[Check]) {
    let passed = checks.iter().filter(|c| c.ok).count();
    let failed = checks.len().saturating_sub(passed);

    blank_lines(2);
    render_section("setup complete");
    render_kv("files", &generated.len().to_string());
    render_kv("checks", &format!("{passed} passed / {failed} failed"));

    blank_lines(1);
    render_section("generated files");
    render_generated_files(generated);

    blank_lines(1);
    render_section("validation");
    for c in checks {
        let mark = if c.ok {
            Style::new().cyan().bold().apply_to("[ok]").to_string()
        } else {
            Style::new().red().bold().apply_to("[fail]").to_string()
        };
        eprintln!(
            "  {} {} {} {}",
            Style::new().cyan().apply_to("▌"),
            mark,
            c.name,
            Style::new().dim().apply_to(&c.detail)
        );
    }

    blank_lines(1);
    render_section("next step");
    eprintln!(
        "  {} Open this repository in your AI CLI, then run the {} skill.",
        Style::new().cyan().apply_to("▌"),
        Style::new().cyan().bold().apply_to("kickoff")
    );
    eprintln!(
        "  {} The kickoff skill will guide deeper project shaping from the generated target harness.",
        Style::new().cyan().apply_to("▌")
    );
    blank_lines(1);
}

/// `.owox/` が無い repo へ最小正本を初期化する。
///
/// 初回 setup は最小入力だけ聞く。詳しい性質・ガードレールは kickoff で人間確認しながら
/// 固める (`docs/decisions/20260621-Phase9-経験層スケールとGitHub連携とkickoff束ね.md`)。
fn bootstrap_canon(base: &Path, owox_dir: &Path) -> Result<(), String> {
    let available = target_names();
    if available.is_empty() {
        return Err("no target CLIs are registered".to_string());
    }
    let answers = prompt_bootstrap(base, &available)?;
    bootstrap_canon_with_answers(base, &answers)?;
    blank_lines(1);
    render_section("canon initialized");
    render_kv("path", &format_generated_path(owox_dir));
    Ok(())
}

fn bootstrap_canon_with_answers(base: &Path, answers: &BootstrapAnswers) -> Result<(), String> {
    let owox_dir = base.join(".owox");
    fs::create_dir_all(&owox_dir).map_err(|err| {
        format!(
            "could not create {}: {err}",
            format_generated_path(&owox_dir)
        )
    })?;

    let brand_path = owox_dir.join("brand.md");
    if brand_path.exists() {
        return Err(format!(
            "{} already exists; review the existing .owox/ canon before running setup again",
            format_generated_path(&brand_path)
        ));
    }
    fs::write(&brand_path, render_brand_md(&answers.vision)).map_err(|err| {
        format!(
            "could not write {}: {err}",
            format_generated_path(&brand_path)
        )
    })?;

    let config_path = owox_dir.join("config.toml");
    if !config_path.exists() {
        fs::write(&config_path, render_config_toml(&answers.targets)).map_err(|err| {
            format!(
                "could not write {}: {err}",
                format_generated_path(&config_path)
            )
        })?;
    }
    Ok(())
}

fn render_brand_md(vision: &str) -> String {
    format!("# Brand\n\n## Vision\n\n{}\n", vision.trim())
}

fn render_config_toml(targets: &[String]) -> String {
    let mut out = String::new();
    for (i, target) in targets.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        out.push_str(&format!("[targets.{target}]\nout = \".\"\n"));
    }
    out
}

fn target_names() -> Vec<String> {
    owox_core::registry()
        .into_iter()
        .map(|t| t.name().to_string())
        .collect()
}

fn prompt_bootstrap(base: &Path, available: &[String]) -> Result<BootstrapAnswers, String> {
    render_welcome(base, available);
    let targets = prompt_targets(available)?;
    let vision = prompt_nonempty("Project vision", None)?;
    let answers = BootstrapAnswers { targets, vision };
    confirm_bootstrap(base, &answers)?;
    Ok(answers)
}

fn render_welcome(base: &Path, available: &[String]) {
    let cyan = Style::new().cyan();
    let bold_cyan = Style::new().cyan().bold();
    eprintln!();
    render_logo();
    eprintln!();
    render_section("workspace");
    render_kv("repo", &base.display().to_string());
    render_kv("canon", &format!("{}/.owox/", base.display()));
    render_kv("clis", &available.join(", "));
    eprintln!();
    render_section("setup flow");
    eprintln!(
        "  {} {} {}",
        cyan.apply_to("▌"),
        cyan.apply_to("01"),
        bold_cyan.apply_to("create minimal .owox/ canon")
    );
    eprintln!(
        "  {} {} {}",
        cyan.apply_to("▌"),
        cyan.apply_to("02"),
        bold_cyan.apply_to("generate selected target harness files")
    );
    eprintln!(
        "  {} {} {}",
        cyan.apply_to("▌"),
        cyan.apply_to("03"),
        bold_cyan.apply_to("handoff to kickoff for deeper shaping")
    );
    eprintln!();
}

fn prompt_targets(available: &[String]) -> Result<Vec<String>, String> {
    render_step_header(
        "01",
        "targets",
        "move: arrows/jk  ·  toggle: Space/x/number  ·  confirm: Enter",
    );
    blank_lines(2);

    let term = Term::stderr();
    let mut cursor = 0usize;
    let mut checked = vec![false; available.len()];
    let mut message = String::new();
    let lines = 7 + available.len();
    let mut rendered = false;

    loop {
        if rendered {
            term.clear_last_lines(lines)
                .map_err(|err| format!("could not redraw target selector: {err}"))?;
        }
        render_target_selector(available, &checked, cursor, &message);
        rendered = true;

        match term
            .read_key()
            .map_err(|err| format!("could not read target selection: {err}"))?
        {
            Key::ArrowDown | Key::Char('j') => {
                cursor = (cursor + 1) % available.len();
                message.clear();
            }
            Key::ArrowUp | Key::Char('k') => {
                cursor = (cursor + available.len() - 1) % available.len();
                message.clear();
            }
            Key::Char(' ') | Key::Char('x') | Key::Char('X') => {
                checked[cursor] = !checked[cursor];
                message.clear();
            }
            Key::Char('a') | Key::Char('A') => {
                let select_all = !checked.iter().all(|v| *v);
                checked.fill(select_all);
                message.clear();
            }
            Key::Char(ch) if ch.is_ascii_digit() => {
                if let Some(index) = ch.to_digit(10).and_then(|n| n.checked_sub(1)) {
                    let index = index as usize;
                    if index < checked.len() {
                        checked[index] = !checked[index];
                        cursor = index;
                        message.clear();
                    }
                }
            }
            Key::Enter => {
                let targets: Vec<String> = checked
                    .iter()
                    .enumerate()
                    .filter_map(|(i, is_checked)| {
                        if *is_checked {
                            available.get(i).cloned()
                        } else {
                            None
                        }
                    })
                    .collect();
                if targets.is_empty() {
                    message = "select at least one target".to_string();
                    continue;
                }
                term.clear_last_lines(lines)
                    .map_err(|err| format!("could not redraw target selector: {err}"))?;
                term.show_cursor()
                    .map_err(|err| format!("could not show cursor: {err}"))?;
                eprintln!(
                    "  {} {}",
                    Style::new().cyan().apply_to("✓"),
                    Style::new()
                        .cyan()
                        .bold()
                        .apply_to(format!("targets: {}", targets.join(", ")))
                );
                return Ok(targets);
            }
            Key::Escape | Key::CtrlC | Key::Char('q') | Key::Char('Q') => {
                return Err("cancelled".to_string());
            }
            _ => {}
        }
    }
}

fn prompt_nonempty(label: &str, default: Option<&str>) -> Result<String, String> {
    eprintln!();
    render_step_header("02", "vision", "one line that anchors the generated canon");
    blank_lines(2);
    loop {
        Term::stderr()
            .show_cursor()
            .map_err(|err| format!("could not show cursor: {err}"))?;
        let theme = cyan_theme();
        let mut input = Input::<String>::with_theme(&theme).with_prompt(label);
        if let Some(default) = default {
            input = input.default(default.to_string());
        }
        let answer = input
            .interact_text()
            .map_err(|err| format!("could not read input: {err}"))?;
        if !answer.trim().is_empty() {
            blank_lines(2);
            return Ok(answer.trim().to_string());
        }
        eprintln!("owox setup: value is required");
    }
}

fn confirm_bootstrap(base: &Path, answers: &BootstrapAnswers) -> Result<(), String> {
    let cyan = Style::new().cyan();
    let bold_cyan = Style::new().cyan().bold();
    eprintln!();
    render_step_header("03", "review", "final check before writing files");
    eprintln!();
    render_section("summary");
    render_kv("repo", &base.display().to_string());
    render_kv("clis", &answers.targets.join(", "));
    render_kv("vision", &answers.vision);
    eprintln!();
    eprintln!(
        "  {} {}",
        cyan.apply_to("◆"),
        bold_cyan.apply_to("files to write")
    );
    for path in preview_paths(base, answers) {
        eprintln!("    {} {}", cyan.apply_to("└─"), path.display());
    }
    eprintln!();
    let ok = Confirm::with_theme(&cyan_theme())
        .with_prompt("Create this setup")
        .default(true)
        .interact()
        .map_err(|err| format!("could not read confirmation: {err}"))?;
    if ok {
        Ok(())
    } else {
        Err("cancelled".to_string())
    }
}

fn render_logo() {
    let logo = [
        r"    ____  __        __ ____  __  __",
        r"   / __ \ \ \      / // __ \ \ \/ /",
        r"  | |  | | \ \ /\ / /| |  | | >  < ",
        r"  | |__| |  \ V  V / | |__| |/ /\ \",
        r"   \____/    \_/\_/   \____//_/  \_\",
        r"             h a r n e s s",
    ];
    let styles = [
        Style::new().cyan().bold(),
        Style::new().cyan(),
        Style::new().blue(),
        Style::new().green(),
        Style::new().cyan().bold(),
        Style::new().dim(),
    ];
    let cyan = Style::new().cyan();
    let dim = Style::new().dim();
    eprintln!(
        "  {} {}",
        cyan.apply_to("▌"),
        Style::new().cyan().bold().apply_to("owox-harness")
    );
    eprintln!("  {} {}", cyan.apply_to("▌"), dim.apply_to("setup wizard"));
    eprintln!("  {}", cyan.apply_to("▌"));
    for (i, line) in logo.iter().enumerate() {
        eprintln!("  {} {}", cyan.apply_to("▌"), styles[i].apply_to(*line));
    }
    eprintln!(
        "  {} {}",
        cyan.apply_to("▌"),
        Style::new()
            .cyan()
            .bold()
            .apply_to("Human control. Agent autonomy.")
    );
}

fn render_step_header(step: &str, title: &str, hint: &str) {
    let cyan = Style::new().cyan();
    eprintln!(
        "  {} {} {} {}",
        cyan.apply_to("▌"),
        Style::new()
            .cyan()
            .bold()
            .apply_to(format!("step {step}/03")),
        Style::new().dim().apply_to("·"),
        Style::new().cyan().bold().apply_to(title)
    );
    eprintln!(
        "  {} {} {}",
        cyan.apply_to("▌"),
        Style::new().dim().apply_to("hint"),
        hint
    );
}

fn render_target_selector(available: &[String], checked: &[bool], cursor: usize, message: &str) {
    let cyan = Style::new().cyan();
    let dim = Style::new().dim();
    blank_lines(2);
    eprintln!(
        "  {} {}",
        cyan.apply_to("▌"),
        Style::new()
            .cyan()
            .bold()
            .apply_to("Target CLIs to generate")
    );
    eprintln!(
        "  {} {}",
        cyan.apply_to("▌"),
        dim.apply_to("Space toggles current item. Numbers also toggle directly.")
    );
    for (i, name) in available.iter().enumerate() {
        let pointer = if i == cursor {
            cyan.apply_to(">").to_string()
        } else {
            " ".to_string()
        };
        let mark = if checked.get(i).copied().unwrap_or(false) {
            cyan.apply_to("[x]").to_string()
        } else {
            dim.apply_to("[ ]").to_string()
        };
        eprintln!(
            "  {} {} {} {} {}",
            cyan.apply_to("▌"),
            pointer,
            mark,
            i + 1,
            name
        );
    }
    if message.is_empty() {
        eprintln!("  {}", cyan.apply_to("▌"));
    } else {
        eprintln!(
            "  {} {}",
            cyan.apply_to("▌"),
            Style::new().red().apply_to(message)
        );
    }
    blank_lines(2);
}

fn render_section(title: &str) {
    eprintln!(
        "  {} {}",
        Style::new().cyan().apply_to("▌"),
        Style::new().cyan().bold().apply_to(title)
    );
}

fn render_kv(label: &str, value: &str) {
    eprintln!(
        "  {} {} {}",
        Style::new().cyan().apply_to("▌"),
        Style::new().dim().apply_to(format!("{label:<6}")),
        value
    );
}

fn render_generated_files(generated: &[(PathBuf, GeneratedFile)]) {
    let mut groups: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (path, _) in generated {
        let formatted = format_generated_path(path);
        groups
            .entry(path_group(&formatted).to_string())
            .or_default()
            .push(formatted);
    }

    for (group, paths) in groups {
        eprintln!(
            "  {} {} {}",
            Style::new().cyan().apply_to("▌"),
            Style::new().cyan().bold().apply_to(group),
            Style::new().dim().apply_to(format!("({})", paths.len()))
        );
        for path in paths {
            eprintln!(
                "  {}   {}",
                Style::new().cyan().apply_to("▌"),
                Style::new().dim().apply_to(path)
            );
        }
        eprintln!("  {}", Style::new().cyan().apply_to("▌"));
    }
}

fn path_group(path: &str) -> &str {
    if let Some(rest) = path.strip_prefix("./") {
        return path_group(rest);
    }
    match path.split('/').next().filter(|part| !part.is_empty()) {
        Some(group) => group,
        None => ".",
    }
}

fn format_generated_path(path: &Path) -> String {
    let mut out = PathBuf::new();
    let mut has_component = false;
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::Normal(part) => {
                out.push(part);
                has_component = true;
            }
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                out.push(component.as_os_str());
                has_component = true;
            }
        }
    }
    if has_component {
        out.display().to_string()
    } else {
        ".".to_string()
    }
}

fn blank_lines(count: usize) {
    for _ in 0..count {
        eprintln!();
    }
}

fn cyan_theme() -> ColorfulTheme {
    ColorfulTheme {
        defaults_style: Style::new().cyan(),
        prompt_style: Style::new().cyan().bold(),
        prompt_prefix: style("?".to_string()).cyan().bold(),
        prompt_suffix: style("›".to_string()).cyan(),
        success_prefix: style("✓".to_string()).cyan(),
        success_suffix: style("".to_string()).cyan(),
        error_prefix: style("✗".to_string()).red(),
        error_style: Style::new().red(),
        hint_style: Style::new().dim(),
        values_style: Style::new().cyan(),
        active_item_style: Style::new().cyan().bold(),
        inactive_item_style: Style::new(),
        active_item_prefix: style(">".to_string()).cyan().bold(),
        inactive_item_prefix: style(" ".to_string()).dim(),
        checked_item_prefix: style("[x]".to_string()).cyan(),
        unchecked_item_prefix: style("[ ]".to_string()).dim(),
        picked_item_prefix: style("◆".to_string()).cyan(),
        unpicked_item_prefix: style("◇".to_string()).dim(),
    }
}

fn preview_paths(base: &Path, answers: &BootstrapAnswers) -> Vec<PathBuf> {
    let mut out = vec![base.join(".owox/brand.md"), base.join(".owox/config.toml")];
    let canon = bootstrap_preview_canon(answers);
    for target_name in &answers.targets {
        let Some(target) = owox_core::find(target_name) else {
            continue;
        };
        for file in target.generate(&canon) {
            out.push(base.join(file.path));
        }
    }
    out.sort();
    out.dedup();
    out
}

fn bootstrap_preview_canon(answers: &BootstrapAnswers) -> Canon {
    Canon {
        brand: Brand {
            vision: answers.vision.clone(),
            ..Brand::default()
        },
        targets: Targets {
            entries: answers
                .targets
                .iter()
                .map(|name| TargetSpec {
                    name: name.clone(),
                    out_dir: ".".to_string(),
                    models: Vec::new(),
                })
                .collect(),
        },
        ..Canon::default()
    }
}

#[cfg(test)]
fn choose_targets(available: &[String], answer: &str) -> Option<Vec<String>> {
    let trimmed = answer.trim();
    if trimmed.is_empty() {
        return available.first().map(|name| vec![name.clone()]);
    }

    let mut selected = Vec::new();
    for piece in trimmed.split(',') {
        let token = piece.trim();
        if token.is_empty() {
            continue;
        }
        let name = if let Ok(n) = token.parse::<usize>() {
            available.get(n.saturating_sub(1)).cloned()
        } else {
            available.iter().find(|name| *name == token).cloned()
        }?;
        if !selected.contains(&name) {
            selected.push(name);
        }
    }
    if selected.is_empty() {
        None
    } else {
        Some(selected)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// std だけで作る一意な一時ディレクトリ (記録層テストと同方式)。
    fn tempdir() -> PathBuf {
        use std::sync::atomic::{AtomicU32, Ordering};
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let pid = std::process::id();
        let dir = std::env::temp_dir().join(format!("owox-setup-test-{pid}-{n}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write(dir: &Path, name: &str, body: &str) -> PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, body).unwrap();
        path
    }

    #[test]
    fn valid_toml_and_json_pass() {
        let dir = tempdir();
        let toml = write(
            &dir,
            "config.toml",
            "[mcp_servers.owox]\ncommand = \"owox\"\n",
        );
        let json = write(&dir, "hooks.json", "{\"hooks\": {}}");
        assert!(check_generated_file(&toml).unwrap().ok);
        assert!(check_generated_file(&json).unwrap().ok);
    }

    #[test]
    fn broken_toml_and_json_fail() {
        let dir = tempdir();
        let toml = write(&dir, "config.toml", "this = = broken");
        let json = write(&dir, "hooks.json", "{not json");
        assert!(!check_generated_file(&toml).unwrap().ok);
        assert!(!check_generated_file(&json).unwrap().ok);
    }

    #[test]
    fn non_config_extension_is_not_checked() {
        let dir = tempdir();
        let md = write(&dir, "AGENTS.md", "# anything");
        // 設定ファイルでない生成物 (Markdown 等) は妥当性検査の対象外。
        assert!(check_generated_file(&md).is_none());
    }

    #[test]
    fn choose_targets_accepts_default_index_name_and_multiple() {
        let available = vec!["codex".to_string(), "claude".to_string()];
        assert_eq!(
            choose_targets(&available, ""),
            Some(vec!["codex".to_string()])
        );
        assert_eq!(
            choose_targets(&available, "2"),
            Some(vec!["claude".to_string()])
        );
        assert_eq!(
            choose_targets(&available, "claude"),
            Some(vec!["claude".to_string()])
        );
        assert_eq!(
            choose_targets(&available, "1,claude,1"),
            Some(vec!["codex".to_string(), "claude".to_string()])
        );
        assert_eq!(choose_targets(&available, "bogus"), None);
    }

    #[test]
    fn format_generated_path_removes_repeated_current_dirs() {
        assert_eq!(
            format_generated_path(Path::new("././.codex/config.toml")),
            ".codex/config.toml"
        );
        assert_eq!(format_generated_path(Path::new("./.owox")), ".owox");
    }

    #[test]
    fn bootstrap_writes_minimal_canon() {
        let dir = tempdir();
        bootstrap_canon_with_answers(
            &dir,
            &BootstrapAnswers {
                targets: vec!["codex".to_string(), "claude".to_string()],
                vision: "Ship useful software.".to_string(),
            },
        )
        .unwrap();

        let brand = std::fs::read_to_string(dir.join(".owox/brand.md")).unwrap();
        let config = std::fs::read_to_string(dir.join(".owox/config.toml")).unwrap();
        assert!(brand.contains("Ship useful software."));
        assert!(config.contains("[targets.codex]"));
        assert!(config.contains("[targets.claude]"));

        let canon = owox_core::load_canon(&dir.join(".owox")).unwrap();
        assert_eq!(canon.brand.vision, "Ship useful software.");
        assert_eq!(canon.targets.entries.len(), 2);
        assert_eq!(canon.targets.entries[0].name, "claude");
        assert_eq!(canon.targets.entries[1].name, "codex");
    }

    #[test]
    fn preview_lists_owox_and_target_files() {
        let dir = tempdir();
        let paths = preview_paths(
            &dir,
            &BootstrapAnswers {
                targets: vec!["codex".to_string()],
                vision: "v".to_string(),
            },
        );
        assert!(paths.contains(&dir.join(".owox/brand.md")));
        assert!(paths.contains(&dir.join(".owox/config.toml")));
        assert!(paths.contains(&dir.join(".codex/config.toml")));
        assert!(paths.contains(&dir.join(".codex/hooks.json")));
    }
}
