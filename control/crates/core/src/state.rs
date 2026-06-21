//! state.set: プロジェクト状態 (phase) を宣言する。
//!
//! 状態は人間宣言が正で `.owox/state.toml` に置く (`docs/decisions/20260611-制御方針.md`)。
//! 変更は来歴へ残す (監査。`docs/decisions/20260611-MCP設計.md`)。
//! today は呼び出し側 (mcp) が与える (core は時計を読まず決定論)。

use std::path::Path;

use serde_json::json;

use crate::envelope::Envelope;
use crate::model::{Phase, State};
use crate::record::{DecisionLinks, DecisionStatus, RecordInput, record_decision};

/// state.set。phase を `.owox/state.toml` へ書き、変更を来歴へ残す。
pub fn set_state(owox_dir: &Path, today: &str, phase: Phase) -> Envelope {
    let state = State { phase };
    let path = owox_dir.join("state.toml");
    if let Err(err) = std::fs::create_dir_all(owox_dir) {
        return Envelope::failed(format!("{} を作れない: {err}", owox_dir.display()));
    }
    if let Err(err) = std::fs::write(&path, state.to_toml()) {
        return Envelope::failed(format!("{} へ書けない: {err}", path.display()));
    }

    // 変更を来歴へ (監査)。記録に失敗しても state 自体は書けているので ok を返す。
    let record = record_decision(
        owox_dir,
        today,
        RecordInput {
            title: format!("Set project state to {}", phase.as_str()),
            status: DecisionStatus::Adopted,
            rationale: format!(
                "Project phase declared as {}. This adjusts how strict the machine gates are.",
                phase.as_str()
            ),
            links: DecisionLinks::default(),
            supersedes: Vec::new(),
        },
    );
    let decision_ids = record.decision_ids;

    Envelope::ok(
        format!("Project state set to {}.", phase.as_str()),
        json!({ "phase": phase.as_str() }),
    )
    .with_decision_ids(decision_ids)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::load::load_canon;

    fn tempdir() -> std::path::PathBuf {
        use std::sync::atomic::{AtomicU32, Ordering};
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let pid = std::process::id();
        let dir = std::env::temp_dir().join(format!("owox-state-test-{pid}-{n}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        // brand.md は必須。
        std::fs::write(dir.join("brand.md"), "## Vision\n\nv\n").unwrap();
        dir
    }

    #[test]
    fn set_state_writes_and_records() {
        let dir = tempdir();
        let env = set_state(&dir, "20260613", Phase::Maintenance);
        assert_eq!(env.status, crate::envelope::Status::Ok);
        // state.toml が読み戻せる。
        let canon = load_canon(&dir).unwrap();
        assert_eq!(canon.state.phase, Phase::Maintenance);
        // 来歴が残る。
        assert_eq!(env.decision_ids.len(), 1);
    }
}
