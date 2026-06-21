//! generate サブコマンド。正本 `.owox/` から target harness を生成し配置する。
//!
//! 決定論ロジックは core に集める。ここは入出力の配線のみ。

use std::path::{Path, PathBuf};
use std::process::ExitCode;

/// `owox generate <target> [dir]` を捌く。
///
/// `dir` は target repo ルート (既定はカレント)。`dir/.owox/` を読み、
/// 生成物を `dir/` 下へ書く。
pub fn run(args: &[String]) -> ExitCode {
    let Some(target_name) = args.first() else {
        eprintln!("owox generate: 対象 CLI が必要 (例: codex)");
        return ExitCode::from(2);
    };

    let base: PathBuf = args
        .get(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));

    let Some(target) = owox_core::find(target_name) else {
        eprintln!("owox generate: 未知の対象 CLI: {target_name}");
        return ExitCode::from(2);
    };

    let owox_dir = base.join(".owox");
    let canon = match owox_core::load_canon(&owox_dir) {
        Ok(canon) => canon,
        Err(err) => {
            eprintln!("owox generate: 正本を読めない: {err}");
            return ExitCode::FAILURE;
        }
    };

    let files = target.generate(&canon);
    if let Err(err) = owox_core::write_all(&base, &files) {
        eprintln!("owox generate: {err}");
        return ExitCode::FAILURE;
    }

    report(&base, &files);
    ExitCode::SUCCESS
}

/// 生成したパスを人間向けに列挙する。
fn report(base: &Path, files: &[owox_core::GeneratedFile]) {
    eprintln!("owox generate: {} 件生成", files.len());
    for file in files {
        eprintln!("  {}", base.join(&file.path).display());
    }
}
