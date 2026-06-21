//! 正本ソース `.owox/` の読込と型検証。
//!
//! 正本は Markdown。読込後に各型の `from_markdown` で検証する。

use std::path::{Path, PathBuf};

use crate::agents::Agents;
use crate::model::{
    Brand, Canon, Context, Glossary, Rules, Settings, State, Targets, VerifyConfig,
};
use crate::profile::Profile;
use crate::quality::Quality;
use crate::release::Release;

/// 正本読込の失敗。読み手が原因を特定できる形で返す。
#[derive(Debug)]
pub enum LoadError {
    /// ファイルが読めない。
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    /// Markdown を型へ検証できない (必須欠落・未知見出し等)。
    Parse { path: PathBuf, message: String },
}

impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LoadError::Read { path, source } => {
                write!(f, "{} を読めない: {source}", path.display())
            }
            LoadError::Parse { path, message } => {
                write!(f, "{} を解釈できない: {message}", path.display())
            }
        }
    }
}

impl std::error::Error for LoadError {}

/// `.owox/` ディレクトリから正本を読む。
///
/// brand.md は必須。rules.md / context.md / glossary.md (Markdown) ・config.toml は任意 (無ければ空)。
pub fn load_canon(owox_dir: &Path) -> Result<Canon, LoadError> {
    let brand_path = owox_dir.join("brand.md");
    let brand = Brand::from_markdown(&read_required(&brand_path)?)
        .map_err(|message| parse_err(&brand_path, message))?;

    let rules_path = owox_dir.join("rules.md");
    let rules = match read_optional(&rules_path)? {
        Some(text) => {
            Rules::from_markdown(&text).map_err(|message| parse_err(&rules_path, message))?
        }
        None => Rules::default(),
    };

    let context_path = owox_dir.join("context.md");
    let context = match read_optional(&context_path)? {
        Some(text) => {
            Context::from_markdown(&text).map_err(|message| parse_err(&context_path, message))?
        }
        None => Context::default(),
    };

    let glossary_path = owox_dir.join("glossary.md");
    let glossary = match read_optional(&glossary_path)? {
        Some(text) => {
            Glossary::from_markdown(&text).map_err(|message| parse_err(&glossary_path, message))?
        }
        None => Glossary::default(),
    };

    let practices_path = owox_dir.join("practices.md");
    let practices = match read_optional(&practices_path)? {
        Some(text) => crate::model::Practices::from_markdown(&text)
            .map_err(|message| parse_err(&practices_path, message))?,
        None => crate::model::Practices::default(),
    };

    let config_path = owox_dir.join("config.toml");
    let config_text = read_optional(&config_path)?;
    let targets = match &config_text {
        Some(text) => {
            Targets::from_toml(text).map_err(|message| parse_err(&config_path, message))?
        }
        None => Targets::default(),
    };
    let verify = match &config_text {
        Some(text) => {
            VerifyConfig::from_toml(text).map_err(|message| parse_err(&config_path, message))?
        }
        None => VerifyConfig::default(),
    };
    let settings = match &config_text {
        Some(text) => {
            Settings::from_toml(text).map_err(|message| parse_err(&config_path, message))?
        }
        None => Settings::default(),
    };

    let state_path = owox_dir.join("state.toml");
    let state = match read_optional(&state_path)? {
        Some(text) => State::from_toml(&text).map_err(|message| parse_err(&state_path, message))?,
        None => State::default(),
    };

    let quality_path = owox_dir.join("quality.toml");
    let quality = match read_optional(&quality_path)? {
        Some(text) => {
            Quality::from_toml(&text).map_err(|message| parse_err(&quality_path, message))?
        }
        None => Quality::default(),
    };

    let profile_path = owox_dir.join("profile.toml");
    let profile = match read_optional(&profile_path)? {
        Some(text) => {
            Profile::from_toml(&text).map_err(|message| parse_err(&profile_path, message))?
        }
        None => Profile::default(),
    };

    let agents_path = owox_dir.join("agents.toml");
    let agents = match read_optional(&agents_path)? {
        Some(text) => {
            Agents::from_toml(&text).map_err(|message| parse_err(&agents_path, message))?
        }
        None => Agents::default(),
    };

    let release_path = owox_dir.join("release.toml");
    let release = match read_optional(&release_path)? {
        Some(text) => {
            Release::from_toml(&text).map_err(|message| parse_err(&release_path, message))?
        }
        None => Release::default(),
    };

    Ok(Canon {
        brand,
        rules,
        context,
        glossary,
        practices,
        targets,
        verify,
        quality,
        state,
        settings,
        profile,
        agents,
        release,
    })
}

/// 必須ファイルの本文を読む。
fn read_required(path: &Path) -> Result<String, LoadError> {
    std::fs::read_to_string(path).map_err(|source| LoadError::Read {
        path: path.to_path_buf(),
        source,
    })
}

/// 任意ファイルの本文を読む。無ければ None。読めるが他の理由で失敗ならエラー。
fn read_optional(path: &Path) -> Result<Option<String>, LoadError> {
    match std::fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(source) => Err(LoadError::Read {
            path: path.to_path_buf(),
            source,
        }),
    }
}

fn parse_err(path: &Path, message: String) -> LoadError {
    LoadError::Parse {
        path: path.to_path_buf(),
        message,
    }
}
