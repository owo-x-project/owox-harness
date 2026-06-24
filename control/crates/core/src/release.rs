//! release.toml: 配布運用がある対象プロジェクト向けの任意正本。
//! 配布方針 / 版 / 成果物検証 (`docs/decisions/20260621-Phase10-配布とrelease正本.md`)。
//!
//! owox は hash を計算せず構文解析も持たない (`crates/core/src/quality.rs` と同じ思想)。
//! 版の取り出し (正規表現) と成果物の存在確認だけ直接行い、checksum / 署名の実検証は
//! 対象プロジェクトのコマンドへ委譲する。無ければ配布運用なし (opt-in)。

use regex::Regex;
use serde::Deserialize;

use crate::model::VerifyCheck;

/// release.toml の型付き表現。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Release {
    /// 配布方針。案内のみ (機械強制しない)。
    pub policy: Vec<String>,
    /// 版の在処。owox が現在の版を読み取る。無ければ版確認を行わない。
    pub version: Option<VersionSource>,
    /// 期待する成果物名。dist 内の存在を確認する。
    pub artifacts: Vec<String>,
    /// 成果物検証の委譲コマンド (checksum / 署名)。verify.checks と同方式。
    pub checks: Vec<VerifyCheck>,
}

/// 版の在処。file の本文から pattern (捕捉群1つ) で版を取り出す。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionSource {
    pub file: String,
    pub pattern: String,
}

impl Release {
    /// release.toml を読み型へ検証する。未知キーと version の不正な正規表現を弾く。
    pub fn from_toml(text: &str) -> Result<Release, String> {
        let raw: ReleaseRaw = toml::from_str(text).map_err(|e| e.to_string())?;

        let version = match raw.version {
            Some(v) => {
                // pattern を読込時に検証する (irreversible の detect: と同じく誤記を早期に弾く)。
                let re = Regex::new(&v.pattern).map_err(|e| {
                    format!("version の pattern 正規表現が不正: {}: {e}", v.pattern)
                })?;
                if re.captures_len() < 2 {
                    return Err(format!(
                        "version の pattern は版を取り出す捕捉群を1つ持つこと: {}",
                        v.pattern
                    ));
                }
                Some(VersionSource {
                    file: v.file,
                    pattern: v.pattern,
                })
            }
            None => None,
        };

        Ok(Release {
            policy: raw.policy,
            version,
            artifacts: raw.artifacts.into_iter().map(|a| a.name).collect(),
            checks: raw
                .checks
                .into_iter()
                .map(|c| VerifyCheck {
                    name: c.name,
                    command: c.command,
                })
                .collect(),
        })
    }

    /// version.file の本文から版を取り出す。pattern の捕捉群1つ目を返す。
    /// version 未設定・不一致なら None。
    pub fn extract_version(&self, file_text: &str) -> Option<String> {
        let v = self.version.as_ref()?;
        // 読込時に妥当性を検証済みのため、ここで失敗しても None で流す。
        let re = Regex::new(&v.pattern).ok()?;
        re.captures(file_text)
            .and_then(|c| c.get(1))
            .map(|m| m.as_str().to_string())
    }

    /// 期待成果物のうち present に無いものを返す (列挙順を保つ)。
    pub fn missing_artifacts<'a>(&'a self, present: &[String]) -> Vec<&'a str> {
        self.artifacts
            .iter()
            .filter(|name| !present.iter().any(|p| p == *name))
            .map(String::as_str)
            .collect()
    }
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct ReleaseRaw {
    #[serde(default)]
    policy: Vec<String>,
    version: Option<VersionRaw>,
    #[serde(default)]
    artifacts: Vec<ArtifactRaw>,
    #[serde(default)]
    checks: Vec<ReleaseCheckRaw>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct VersionRaw {
    file: String,
    pattern: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ArtifactRaw {
    name: String,
}

/// `[[checks]]` の生表現。model の VerifyCheckRaw と同じ流儀で解釈する。
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReleaseCheckRaw {
    name: String,
    command: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_is_default() {
        let r = Release::from_toml("").unwrap();
        assert_eq!(r, Release::default());
    }

    #[test]
    fn reads_policy_version_artifacts_checks() {
        let r = Release::from_toml(
            "policy = [\"tag は owox-v<version>\"]\n\
             [version]\n\
             file = \"Cargo.toml\"\n\
             pattern = '^version = \"(.+)\"'\n\
             [[artifacts]]\n\
             name = \"owox-x86_64-unknown-linux-musl.tar.gz\"\n\
             [[checks]]\n\
             name = \"sha256\"\n\
             command = \"sha256sum -c SHA256SUMS\"\n",
        )
        .unwrap();
        assert_eq!(r.policy, vec!["tag は owox-v<version>".to_string()]);
        assert_eq!(r.version.as_ref().unwrap().file, "Cargo.toml");
        assert_eq!(r.artifacts.len(), 1);
        assert_eq!(r.checks.len(), 1);
        assert_eq!(r.checks[0].name, "sha256");
    }

    #[test]
    fn unknown_key_is_rejected() {
        let err = Release::from_toml("bogus = 1\n").unwrap_err();
        assert!(!err.is_empty());
    }

    #[test]
    fn invalid_version_pattern_is_rejected() {
        let err =
            Release::from_toml("[version]\nfile = \"x\"\npattern = \"(unclosed\"\n").unwrap_err();
        assert!(err.contains("pattern"), "{err}");
    }

    #[test]
    fn version_pattern_without_capture_is_rejected() {
        let err =
            Release::from_toml("[version]\nfile = \"x\"\npattern = \"^version\"\n").unwrap_err();
        assert!(err.contains("捕捉群"), "{err}");
    }

    #[test]
    fn extract_version_returns_capture() {
        // 行頭で取り出すには pattern に複数行モード (?m) を付ける (regex の流儀)。
        let r = Release::from_toml(
            "[version]\nfile = \"Cargo.toml\"\npattern = '(?m)^version = \"(.+)\"'\n",
        )
        .unwrap();
        let got = r.extract_version("[package]\nname = \"x\"\nversion = \"0.1.0\"\n");
        assert_eq!(got.as_deref(), Some("0.1.0"));
    }

    #[test]
    fn extract_version_none_without_match() {
        let r = Release::from_toml(
            "[version]\nfile = \"Cargo.toml\"\npattern = '(?m)^version = \"(.+)\"'\n",
        )
        .unwrap();
        assert_eq!(r.extract_version("no version here"), None);
    }

    #[test]
    fn missing_artifacts_reports_absent() {
        let r = Release::from_toml(
            "[[artifacts]]\nname = \"a.tar.gz\"\n[[artifacts]]\nname = \"b.zip\"\n",
        )
        .unwrap();
        let present = vec!["a.tar.gz".to_string()];
        assert_eq!(r.missing_artifacts(&present), vec!["b.zip"]);
    }
}
