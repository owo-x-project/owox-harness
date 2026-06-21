//! 秘密情報の走査。owox 同梱の既定パターンでテキストを照合する
//! (`docs/decisions/20260614-Phase7-測定可視化とブランド検証.md`)。
//!
//! `irreversible.rs` と同じ「組込み正規表現 + 将来の target 拡張」の型に載せる。
//! スライス3 は文脈出力時の走査、スライス4 は経験 export/import が再利用先。
//!
//! 既定は高価値で誤検出が少ないものだけ持つ (正常テキストを巻き込まない)。
//! 走査結果に秘密の値そのものは載せない (結果が新たな漏れ口にならない)。

use regex::Regex;

/// 検出された秘密情報。
pub struct SecretFinding {
    /// 検出元の識別子 (既定検出器 id)。
    pub id: String,
    /// なぜ危険か。人間・AI へ返す理由。秘密の値は含めない。
    pub detail: String,
}

/// 既定の秘密検出器 1 件。
struct Detector {
    id: &'static str,
    /// テキストへ照合する正規表現。
    pattern: &'static str,
    detail: &'static str,
}

/// 既定の秘密パターン。
///
/// 各パターンは値の形・長さを明示し、prose や識別子を巻き込まない。
/// 汎用の代入は鍵語 + 区切り + 十分な長さの値の時だけ当てる。
const BUILTINS: &[Detector] = &[
    Detector {
        id: "private-key",
        pattern: r"-----BEGIN(?: [A-Z0-9]+)* PRIVATE KEY-----",
        detail: "Looks like a private key block. Do not put it into context.",
    },
    Detector {
        id: "aws-access-key-id",
        pattern: r"\bAKIA[0-9A-Z]{16}\b",
        detail: "Looks like an AWS access key id. Do not put it into context.",
    },
    Detector {
        id: "github-token",
        pattern: r"\bgh[pousr]_[A-Za-z0-9]{20,}\b",
        detail: "Looks like a GitHub access token. Do not put it into context.",
    },
    Detector {
        id: "credential-assignment",
        pattern: r#"(?i)\b(?:api[_-]?key|secret|password|passwd|access[_-]?token|auth[_-]?token|private[_-]?key)\b\s*[:=]\s*["']?[A-Za-z0-9_\-./+]{16,}"#,
        detail: "Looks like a credential assigned to a long value. Do not put it into context.",
    },
];

/// テキストを既定パターンで走査し、当たった秘密を返す。
///
/// 当たった種別ごとに 1 件にまとめる (同種が複数行あっても束ねる)。
/// 当たらなければ空 (素通り)。
pub fn scan(text: &str) -> Vec<SecretFinding> {
    let mut findings = Vec::new();
    for d in BUILTINS {
        // 既定パターンは固定で必ず妥当 (compile_all テストで担保)。
        let re = Regex::new(d.pattern).expect("built-in secret pattern is valid");
        if re.is_match(text) {
            findings.push(SecretFinding {
                id: d.id.to_string(),
                detail: d.detail.to_string(),
            });
        }
    }
    findings
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_builtins_compile() {
        for d in BUILTINS {
            Regex::new(d.pattern).unwrap_or_else(|e| panic!("{}: {e}", d.id));
        }
    }

    #[test]
    fn detects_private_key() {
        let text = "-----BEGIN RSA PRIVATE KEY-----\nMIIE...\n-----END RSA PRIVATE KEY-----";
        let f = scan(text);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].id, "private-key");
        // 値そのものは載せない。
        assert!(!f[0].detail.contains("MIIE"));
    }

    #[test]
    fn detects_aws_access_key() {
        let f = scan("aws_key = AKIAIOSFODNN7EXAMPLE rest");
        assert!(f.iter().any(|x| x.id == "aws-access-key-id"));
    }

    #[test]
    fn detects_github_token() {
        let f = scan("token: ghp_abcdefghijklmnopqrstuvwxyz0123456789");
        assert!(f.iter().any(|x| x.id == "github-token"));
    }

    #[test]
    fn detects_credential_assignment() {
        let f = scan("password = \"s3cr3t_value_1234567890\"");
        assert!(f.iter().any(|x| x.id == "credential-assignment"));
    }

    #[test]
    fn ignores_plain_prose() {
        // 通常の日本語・英語の文や、鍵語が出るだけで値が無い文は巻き込まない。
        assert!(scan("これは文脈地図です。トークン数を表示します。").is_empty());
        assert!(scan("Read the files and apply the rules.").is_empty());
        assert!(scan("The password policy is documented here.").is_empty());
        assert!(scan("api key rotation is a good practice").is_empty());
    }
}
