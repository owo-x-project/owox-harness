//! verify.run: 完了を3区別して返す (`docs/requirements/20260611-core-要件.md`)。
//!
//! - 検証完了: config の検査コマンドを実行し全通過なら passed (機械判定)
//! - 要件完了: accepted な要件ごとに受け入れ基準の検証 link と検査結果から met を導出する
//!   (`docs/decisions/20260614-Phase6-要件完了の機械判定.md`)。trace の無い要件は needs_human
//! - 作業完了: 与えた範囲が終わったかは機械判定できないため needs_human
//!
//! 封筒 status は、検査失敗→failed、それ以外→needs_human (検証・要件は機械判定するが
//! 作業完了は人間確認)。旗は data の completion.requirement が met へ動くことで立つ。

use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;

use serde_json::json;

use crate::decay::DecayFinding;
use crate::envelope::{Envelope, Gate};
use crate::model::VerifyConfig;
use crate::quality::QualityViolation;
use crate::requirement::{Met, Requirement, RequirementStatus};

/// 完了の各区別の状態。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Completion {
    Passed,
    Failed,
    NeedsHuman,
}

impl Completion {
    fn as_str(self) -> &'static str {
        match self {
            Completion::Passed => "passed",
            Completion::Failed => "failed",
            Completion::NeedsHuman => "needs_human",
        }
    }
}

/// 1 検査の結果。
pub struct CheckResult {
    pub name: String,
    pub passed: bool,
    pub detail: String,
}

/// 検査を順に実行し結果を集める。verify.run と commit ゲートが共用する
/// (重複ロジックを core に集約。`docs/decisions/20260611-MCP設計.md`)。
pub fn run_checks(work_dir: &Path, checks: &[crate::model::VerifyCheck]) -> Vec<CheckResult> {
    checks
        .iter()
        .map(|c| CheckResult {
            name: c.name.clone(),
            ..run_check(work_dir, &c.command)
        })
        .collect()
}

/// `dir` で検査コマンドを実行する。シェル経由 (`sh -c`)。
fn run_check(dir: &Path, command: &str) -> CheckResult {
    let output = Command::new("sh")
        .arg("-c")
        .arg(command)
        .current_dir(dir)
        .output();

    match output {
        Ok(out) if out.status.success() => CheckResult {
            name: command.to_string(),
            passed: true,
            detail: "ok".to_string(),
        },
        Ok(out) => {
            let code = out
                .status
                .code()
                .map(|c| c.to_string())
                .unwrap_or_else(|| "signal".to_string());
            let stderr = String::from_utf8_lossy(&out.stderr);
            let tail: String = stderr
                .lines()
                .rev()
                .take(3)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect::<Vec<_>>()
                .join("\n");
            CheckResult {
                name: command.to_string(),
                passed: false,
                detail: format!("exit {code}: {tail}"),
            }
        }
        Err(err) => CheckResult {
            name: command.to_string(),
            passed: false,
            detail: format!("could not run: {err}"),
        },
    }
}

/// accepted な要件を判定し、(集約 met, 要件ごとの詳細 JSON) を返す。
///
/// 集約は failed > needs_human > met の優先順。accepted が無ければ needs_human で第1段階を温存する。
fn judge_requirements(
    requirements: &[Requirement],
    check_passed: &BTreeMap<String, bool>,
) -> (Met, Vec<serde_json::Value>) {
    let mut agg_failed = false;
    let mut agg_needs = false;
    let mut any = false;
    let mut details = Vec::new();

    for r in requirements
        .iter()
        .filter(|r| r.status == RequirementStatus::Accepted)
    {
        any = true;
        let (met, reason) = r.judge(check_passed);
        match met {
            Met::Failed => agg_failed = true,
            Met::NeedsHuman => agg_needs = true,
            Met::Met => {}
        }
        details.push(json!({
            "id": r.id,
            "title": r.title,
            "met": met.as_str(),
            "unlinked": r.unlinked(),
            "reason": reason,
        }));
    }

    let aggregate = if !any {
        Met::NeedsHuman
    } else if agg_failed {
        Met::Failed
    } else if agg_needs {
        Met::NeedsHuman
    } else {
        Met::Met
    };
    (aggregate, details)
}

/// verify.run。`work_dir` で検査を実行し、完了3区別を封筒で返す。
///
/// work_dir は target repo ルート (検査コマンドを実行する場所)。
/// requirements は accepted 要件の met 判定に使う (受け入れ基準の検証 link を検査結果へ照合)。
/// quality_violations は品質バー違反 (mcp が run_quality で組む)。報告のみで完了3区別は変えない
/// (判定は commit ゲートに集約。`docs/decisions/20260614-Phase6-quality適応度関数.md`)。
/// decay_findings も同様に報告のみ (タスク・来歴の腐敗。`docs/decisions/20260614-Phase7-腐敗検知の中核.md`)。
pub fn run_verify(
    config: &VerifyConfig,
    requirements: &[Requirement],
    quality_violations: &[QualityViolation],
    decay_findings: &[DecayFinding],
    work_dir: &Path,
) -> Envelope {
    // 検証完了: 検査コマンドを一度ずつ実行し、結果から判定する。
    let checks = run_checks(work_dir, &config.checks);

    let verification = if checks.is_empty() {
        Completion::NeedsHuman
    } else if checks.iter().all(|r| r.passed) {
        Completion::Passed
    } else {
        Completion::Failed
    };

    let results: Vec<_> = checks
        .iter()
        .map(|r| json!({ "check": r.name, "passed": r.passed, "detail": r.detail }))
        .collect();

    // 検査結果を name→passed の表にし、要件ごとの met を判定する。
    let check_passed: BTreeMap<String, bool> =
        checks.iter().map(|r| (r.name.clone(), r.passed)).collect();
    let (requirement, req_details) = judge_requirements(requirements, &check_passed);

    // 作業完了は機械判定できない。
    let work = Completion::NeedsHuman;

    let completion = json!({
        "work": work.as_str(),
        "requirement": requirement.as_str(),
        "verification": verification.as_str(),
    });
    let quality: Vec<_> = quality_violations
        .iter()
        .map(|v| json!({ "kind": v.kind, "path": v.path, "detail": v.detail }))
        .collect();
    let decay: Vec<_> = decay_findings
        .iter()
        .map(|f| json!({ "kind": f.kind, "subject": f.subject, "detail": f.detail }))
        .collect();
    let data = json!({
        "results": results,
        "completion": completion,
        "requirements": req_details,
        "quality": quality,
        "decay": decay,
    });

    // 封筒 status: 検査失敗のみ failed (要件 failed は link 先検査の失敗を含むのでここに吸収)。
    if verification == Completion::Failed {
        return Envelope::failed(
            "Verification failed: one or more configured checks did not pass.",
        )
        .with_data(data)
        .with_next_actions(vec![
            "Fix the failing checks and run verify again.".to_string(),
        ]);
    }

    // 要件が1件も無いか (accepted 要件ゼロ)。未捕捉と未トレースを取り違えないため案内を分ける。
    let no_accepted_requirements = req_details.is_empty();

    // 要件完了の状態で案内を変える。いずれも作業完了は人間確認のため needs_human。
    let (reason, requires, mut next): (&str, &str, Vec<String>) = match requirement {
        Met::Met => (
            "Checks passed and all accepted requirements are met (machine-judged). Confirm the assigned work scope is complete.",
            "Confirm the assigned work scope is complete.",
            vec!["Confirm the assigned work scope is complete.".to_string()],
        ),
        Met::NeedsHuman if verification == Completion::NeedsHuman => (
            "No verification checks are configured. A human must confirm completion, or add [[verify.checks]] to config.toml.",
            "Confirm work and requirement completion, or configure checks.",
            Vec::new(),
        ),
        Met::NeedsHuman if no_accepted_requirements => (
            "Checks passed and verification is machine-judged. No requirements are captured yet, so a human must confirm work completion, or capture the requirements first. Also confirm the work scope.",
            "Confirm work completion by hand, or capture the requirements first.",
            vec![
                "Capture the requirements with requirement.create, or confirm completion by hand.".to_string(),
                "Confirm the assigned work scope is complete.".to_string(),
            ],
        ),
        _ => (
            "Checks passed (verification done by machine). Some accepted requirements lack a complete verification trace, so requirement completion needs human judgment. Also confirm the work scope.",
            "Link acceptance criteria to checks, or confirm requirement and work completion by hand.",
            vec![
                "Link unverified acceptance criteria to checks (see the requirements list in this result).".to_string(),
                "Confirm the assigned work scope is complete.".to_string(),
            ],
        ),
    };

    // quality 違反は報告のみ (完了3区別は変えない)。気づきとして次の手へ添える。
    if !quality_violations.is_empty() {
        next.push(format!(
            "Address {} quality bar violation(s) (see data.quality); they block commit in maintenance.",
            quality_violations.len()
        ));
    }

    // 腐敗検知も報告のみ。構造的腐敗 (done未検証・ゾンビ) は保守で commit を止める。
    if !decay_findings.is_empty() {
        let structural = decay_findings.iter().filter(|f| f.is_structural()).count();
        next.push(format!(
            "Clean up {} decay finding(s) (see data.decay); {structural} structural one(s) block commit in maintenance.",
            decay_findings.len()
        ));
    }

    Envelope::needs_human(
        reason,
        Gate {
            kind: "completion-judgment".to_string(),
            subject: "work and requirement completion".to_string(),
            requires: requires.to_string(),
        },
    )
    .with_data(data)
    .with_next_actions(next)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::envelope::Status;
    use crate::model::VerifyCheck;

    fn cfg(commands: &[&str]) -> VerifyConfig {
        VerifyConfig {
            checks: commands
                .iter()
                .map(|c| VerifyCheck {
                    name: c.to_string(),
                    command: c.to_string(),
                })
                .collect(),
        }
    }

    #[test]
    fn no_checks_needs_human() {
        let env = run_verify(&VerifyConfig::default(), &[], &[], &[], Path::new("."));
        assert_eq!(env.status, Status::NeedsHuman);
        assert_eq!(
            env.data.unwrap()["completion"]["verification"],
            "needs_human"
        );
    }

    #[test]
    fn passing_checks_need_human_for_work_and_requirement() {
        // 要件が無ければ requirement は needs_human (第1段階互換)。
        let env = run_verify(&cfg(&["true"]), &[], &[], &[], Path::new("."));
        assert_eq!(env.status, Status::NeedsHuman);
        let data = env.data.unwrap();
        assert_eq!(data["completion"]["verification"], "passed");
        assert_eq!(data["completion"]["requirement"], "needs_human");
        assert_eq!(data["completion"]["work"], "needs_human");
        // 要件ゼロは「未捕捉」と案内する。「トレース不足」と取り違えない。
        let reason = env.reason;
        assert!(reason.contains("No requirements are captured yet"), "{reason}");
        assert!(!reason.contains("lack a complete verification trace"), "{reason}");
    }

    #[test]
    fn failing_check_fails() {
        let env = run_verify(&cfg(&["true", "false"]), &[], &[], &[], Path::new("."));
        assert_eq!(env.status, Status::Failed);
        let data = env.data.unwrap();
        assert_eq!(data["completion"]["verification"], "failed");
        let results = data["results"].as_array().unwrap();
        assert_eq!(results.len(), 2);
    }

    /// 受け入れ基準が検査へ link された accepted 要件。
    fn accepted_req(verify: Option<&str>) -> Requirement {
        Requirement {
            id: "20260614-r".to_string(),
            title: "r".to_string(),
            status: RequirementStatus::Accepted,
            statement: String::new(),
            criteria: vec![crate::requirement::AcceptanceCriterion {
                id: 1,
                title: String::new(),
                given: "g".to_string(),
                when: "w".to_string(),
                then: "t".to_string(),
                verify: verify.map(str::to_string),
            }],
            links: crate::requirement::RequirementLinks::default(),
            supersedes: Vec::new(),
            priority: None,
            layer: None,
            stage: None,
            kind: None,
        }
    }

    #[test]
    fn traced_requirement_is_met_when_check_passes() {
        // 検査名 "true" が通り、基準がそこへ link されていれば要件は met (機械判定)。
        let reqs = [accepted_req(Some("true"))];
        let env = run_verify(&cfg(&["true"]), &reqs, &[], &[], Path::new("."));
        assert_eq!(env.status, Status::NeedsHuman); // 作業完了は人間確認
        let data = env.data.unwrap();
        assert_eq!(data["completion"]["requirement"], "met");
        assert_eq!(data["requirements"][0]["met"], "met");
    }

    #[test]
    fn untraced_requirement_stays_needs_human() {
        // 検証 link が欠ける accepted 要件は needs_human のまま (ハイブリッド)。
        let reqs = [accepted_req(None)];
        let env = run_verify(&cfg(&["true"]), &reqs, &[], &[], Path::new("."));
        let data = env.data.unwrap();
        assert_eq!(data["completion"]["verification"], "passed");
        assert_eq!(data["completion"]["requirement"], "needs_human");
        assert_eq!(data["requirements"][0]["unlinked"], 1);
    }
}
