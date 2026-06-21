//! owox 実行ファイル。MCP サーバ用途と hook 補助用途を兼ねる (常駐 CLI ではない)。
//!
//! 現状は generate / setup / hook 補助 / serve (MCP サーバ) / --version。

use std::process::ExitCode;

mod cache;
mod clock;
mod files;
mod generate;
mod hook;
mod serve;
mod setup;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("--version" | "-V") => {
            // 導入物の版確認。配布の checksum 照合後に何を入れたか分かる
            // (`docs/decisions/20260621-Phase10-配布とrelease正本.md`)。
            println!("owox {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        Some("generate") => generate::run(&args[1..]),
        Some("setup") => setup::run(&args[1..]),
        Some("hook") => hook::run(&args[1..]),
        Some("serve") => serve::run(&args[1..]),
        Some(other) => {
            eprintln!("owox: 未知のサブコマンド: {other}");
            ExitCode::from(2)
        }
        None => {
            eprintln!("owox: サブコマンドが必要 (generate / setup / hook / serve)");
            ExitCode::from(2)
        }
    }
}
