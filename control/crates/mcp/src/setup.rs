//! setup サブコマンド。別 target repo へ owox を導入する手順を自動化する。
//!
//! `owox setup [dir]` = 正本読込 → 生成 (canon 駆動・config.toml の targets を各々) → 検査 → 報告。
//! バイナリの install は範囲外 (配布は Phase9。`docs/decisions/20260613-Phase5-スキルと入口.md`)。
//! setup は owox 配置済みを前提に、設定生成と「正しく繋がるか」の検査に絞る。
//!
//! 決定論ロジックは core。ここは入出力の配線と検査の組み立てのみ (generate と同様)。

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use owox_core::GeneratedFile;

/// 検査 1 件の結果。setup の「正しく繋がるか」を人間へ示す。
struct Check {
    name: String,
    ok: bool,
    detail: String,
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
    let canon = match owox_core::load_canon(&owox_dir) {
        Ok(canon) => canon,
        Err(err) => {
            eprintln!("owox setup: 正本を読めない: {err}");
            return ExitCode::FAILURE;
        }
    };

    // 生成対象は canon 駆動。config.toml に targets が無ければ codex を既定にする
    // (第1対象。単一 CLI を最小設定で導入できる)。
    let targets: Vec<(String, String)> = if canon.targets.entries.is_empty() {
        eprintln!("owox setup: config.toml に targets が無いため codex を既定で使う");
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
            eprintln!("owox setup: スキルを読めない: {err}");
            return ExitCode::FAILURE;
        }
    };

    // 入口 (コマンド) を薄い skill として加える。owox 標準 ∪ プロジェクト追加。
    let commands = match owox_core::command_skills(&owox_dir) {
        Ok(skills) => skills,
        Err(err) => {
            eprintln!("owox setup: コマンドを読めない: {err}");
            return ExitCode::FAILURE;
        }
    };

    // 登録済みスキルと入口 skill を一括で生成する (同じ `.agents/skills/` 配置)。
    let mut skills = registered;
    skills.extend(commands);

    let mut generated: Vec<(PathBuf, GeneratedFile)> = Vec::new();
    for (name, out) in &targets {
        let Some(target) = owox_core::find(name) else {
            eprintln!("owox setup: 未知の対象 CLI: {name}");
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
    eprintln!("owox setup: {} 件生成", generated.len());
    for (path, _) in generated {
        eprintln!("  {}", path.display());
    }
    eprintln!("owox setup: 検査");
    for c in checks {
        let mark = if c.ok { "ok" } else { "NG" };
        eprintln!("  [{mark}] {}: {}", c.name, c.detail);
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
}
