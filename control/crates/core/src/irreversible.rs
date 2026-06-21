//! 不可逆操作の検出。owox 同梱の既定パターン + target 記述。
//!
//! PreToolUse hook が Bash コマンドをここへ照合し、当たれば deny する
//! (`docs/decisions/20260612-Phase3-hook実装.md`)。
//! 既定は保守的に高価値なものだけ持つ (誤発火で正常作業を妨げない)。
//! target 固有の不可逆操作は rules.md の `detect:` 正規表現で足せる。

use regex::Regex;

use crate::model::Irreversible;

/// 検出された不可逆操作。
pub struct Detection {
    /// 検出元の識別子 (既定検出器 id、または target の操作名)。
    pub id: String,
    /// なぜ止めるか。人間・AI へ返す理由。
    pub reason: String,
}

/// 既定の不可逆検出器 1 件。
struct Detector {
    id: &'static str,
    /// Bash コマンド全体へ照合する正規表現。
    pattern: &'static str,
    reason: &'static str,
}

/// 既定の不可逆 Bash コマンド。
///
/// 各パターンは語境界と区切りを明示し、安全な近縁コマンドを巻き込まない。
/// 例: `--force` は後続が空白/行末/= の時だけ当て、`--force-with-lease` は除く。
const BUILTINS: &[Detector] = &[
    Detector {
        id: "git-push-force",
        pattern: r"\bgit\s+push\b[^\n]*(--force([ \t=]|$)|[ \t]-f([ \t]|$))",
        reason: "git push --force rewrites remote history and can destroy others' work. Confirm with a human first.",
    },
    Detector {
        id: "git-reset-hard",
        pattern: r"\bgit\s+reset\b[^\n]*--hard\b",
        reason: "git reset --hard discards uncommitted changes irreversibly. Confirm with a human first.",
    },
    Detector {
        id: "rm-recursive-force",
        pattern: r"\brm\b[^\n]*([ \t]-[a-zA-Z]*[rR][a-zA-Z]*f|[ \t]-[a-zA-Z]*f[a-zA-Z]*[rR]|--recursive[^\n]*--force|--force[^\n]*--recursive)",
        reason: "rm with recursive force deletes files permanently. Confirm with a human first.",
    },
    Detector {
        id: "git-clean-force",
        pattern: r"\bgit\s+clean\b[^\n]*[ \t]-[a-zA-Z]*f",
        reason: "git clean -f permanently deletes untracked files. Confirm with a human first.",
    },
    Detector {
        id: "git-branch-force-delete",
        pattern: r"\bgit\s+branch\b[^\n]*([ \t]-D\b|--delete[^\n]*--force|--force[^\n]*--delete)",
        reason: "git branch -D force-deletes a possibly-unmerged branch. Confirm with a human first.",
    },
];

/// Bash コマンドを照合し、最初に当たった不可逆操作を返す。
///
/// owox 同梱の既定パターンを先に、続いて target の `detect:` を見る。
/// target の正規表現は読込時に検証済み (`model::parse_irreversible`)。
/// 当たらなければ None (素通り)。
pub fn detect_bash(command: &str, target: &[Irreversible]) -> Option<Detection> {
    for d in BUILTINS {
        // 既定パターンは固定で必ず妥当 (compile_all テストで担保)。
        let re = Regex::new(d.pattern).expect("built-in irreversible pattern is valid");
        if re.is_match(command) {
            return Some(Detection {
                id: d.id.to_string(),
                reason: d.reason.to_string(),
            });
        }
    }

    for entry in target {
        let Some(pattern) = &entry.detect else {
            continue;
        };
        // 読込時に検証済みだが、念のため失敗は無視 (検出しない) する。
        if let Ok(re) = Regex::new(pattern)
            && re.is_match(command)
        {
            return Some(Detection {
                id: entry.operation.clone(),
                reason: entry.reason.clone(),
            });
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 既定検出器のみ (target 記述なし) で照合する。
    fn builtin(command: &str) -> Option<Detection> {
        detect_bash(command, &[])
    }

    #[test]
    fn all_builtins_compile() {
        for d in BUILTINS {
            Regex::new(d.pattern).unwrap_or_else(|e| panic!("{}: {e}", d.id));
        }
    }

    #[test]
    fn detects_force_push() {
        assert_eq!(builtin("git push --force").unwrap().id, "git-push-force");
        assert_eq!(
            builtin("git push -f origin main").unwrap().id,
            "git-push-force"
        );
        assert_eq!(
            builtin("git push origin main --force").unwrap().id,
            "git-push-force"
        );
    }

    #[test]
    fn allows_safe_push() {
        // 通常 push と force-with-lease (より安全) は止めない。
        assert!(builtin("git push origin main").is_none());
        assert!(builtin("git push --force-with-lease").is_none());
    }

    #[test]
    fn detects_reset_hard() {
        assert_eq!(
            builtin("git reset --hard HEAD~1").unwrap().id,
            "git-reset-hard"
        );
        assert!(builtin("git reset HEAD~1").is_none());
    }

    #[test]
    fn detects_rm_recursive_force() {
        for cmd in [
            "rm -rf build",
            "rm -fr build",
            "rm -Rf build",
            "rm --recursive --force x",
        ] {
            assert_eq!(builtin(cmd).unwrap().id, "rm-recursive-force", "{cmd}");
        }
    }

    #[test]
    fn allows_safe_rm() {
        // 単発削除・非強制再帰は止めない (誤発火回避)。
        assert!(builtin("rm file.txt").is_none());
        assert!(builtin("rm -f file.txt").is_none());
        assert!(builtin("rm -r dir").is_none());
    }

    #[test]
    fn detects_git_clean_force() {
        assert_eq!(builtin("git clean -fdx").unwrap().id, "git-clean-force");
        assert!(builtin("git clean -n").is_none());
    }

    #[test]
    fn detects_branch_force_delete() {
        assert_eq!(
            builtin("git branch -D feature").unwrap().id,
            "git-branch-force-delete"
        );
        assert!(builtin("git branch -d feature").is_none());
    }

    #[test]
    fn allows_unrelated_commands() {
        assert!(builtin("ls -la").is_none());
        assert!(builtin("cargo test").is_none());
        assert!(builtin("git status").is_none());
    }

    #[test]
    fn detects_target_pattern() {
        // target が detect: で足した固有の不可逆操作を検出する。
        let target = vec![Irreversible {
            operation: "terraform destroy".to_string(),
            reason: "tears down all provisioned infrastructure".to_string(),
            detect: Some(r"\bterraform\s+destroy\b".to_string()),
        }];
        let det = detect_bash("terraform destroy -auto-approve", &target).unwrap();
        assert_eq!(det.id, "terraform destroy");
        assert_eq!(det.reason, "tears down all provisioned infrastructure");
        // detect の無いコマンドは素通り。
        assert!(detect_bash("terraform plan", &target).is_none());
    }

    #[test]
    fn target_without_detect_is_not_matched() {
        let target = vec![Irreversible {
            operation: "manual db migration".to_string(),
            reason: "irreversible schema change".to_string(),
            detect: None,
        }];
        assert!(detect_bash("anything at all", &target).is_none());
    }
}
