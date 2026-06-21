//! ゲート合成: 層×phase の直交ゲートを1判定へ畳む
//! (`docs/decisions/20260618-Phase9-性質軸適応機構.md`)。
//!
//! phase ゲート = 遵法の厳しさ (時間軸)。層 ゲート = 変更権限の所在 (性質軸の自律度勾配)。
//! 別種の関心なので独立に発火し AI は両方を通す (AND)。1判定へ畳む時は厳しい方が勝つ (max)。
//! 重症度はしご1本: free < warn < commit-block < pre-action-human-gate。

use crate::model::Phase;
use crate::quality::Autonomy;

/// 重症度のはしご (緩い → 厳しい)。Ord で max を取る。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Enforcement {
    /// 自由。安全ゲートのみ。
    Free,
    /// 警告。助言+記録 (止めない)。
    Warn,
    /// コミット阻止。commit ゲートで block。
    CommitBlock,
    /// 操作前に人間。編集の前に人間承認が要る。
    PreActionHumanGate,
}

/// 層の自律度をはしごへ写像する。
/// guarded は phase 不問で操作前ゲート、supervised は警告 (phase が変調)、free は安全ゲートのみ。
pub fn autonomy_enforcement(autonomy: Autonomy) -> Enforcement {
    match autonomy {
        Autonomy::Guarded => Enforcement::PreActionHumanGate,
        Autonomy::Supervised => Enforcement::Warn,
        Autonomy::Free => Enforcement::Free,
    }
}

/// phase をはしごへ写像する。初期=自由・安定=警告・保守=コミット阻止。
pub fn phase_enforcement(phase: Phase) -> Enforcement {
    match phase {
        Phase::Initial => Enforcement::Free,
        Phase::Stable => Enforcement::Warn,
        Phase::Maintenance => Enforcement::CommitBlock,
    }
}

/// 層写像と phase 写像を1判定へ畳む (厳しい方が勝つ)。
pub fn compose(layer: Enforcement, phase: Enforcement) -> Enforcement {
    layer.max(phase)
}

/// commit 時点で、この自律度・phase の違反が block されるか。
///
/// 境界違反は内容依存で書込後にしか分からないため操作前ゲートにできない。commit で判定する:
/// guarded は phase 不問で block (層が max で勝つ)・supervised/free は phase が保守の時だけ block。
/// 層が無い (architecture=flat 等で全て Free) 時は phase 既存挙動 (保守のみ block) に一致する。
pub fn commit_blocks(autonomy: Autonomy, phase: Phase) -> bool {
    // 操作前ゲートは commit では実現できないため、guarded は commit-block へ落とす。
    let layer = match autonomy {
        Autonomy::Guarded => Enforcement::CommitBlock,
        Autonomy::Supervised => Enforcement::Warn,
        Autonomy::Free => Enforcement::Free,
    };
    compose(layer, phase_enforcement(phase)) >= Enforcement::CommitBlock
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ladder_order() {
        assert!(Enforcement::Free < Enforcement::Warn);
        assert!(Enforcement::Warn < Enforcement::CommitBlock);
        assert!(Enforcement::CommitBlock < Enforcement::PreActionHumanGate);
    }

    // 設計の検算4ケース (max が矛盾しない)。
    #[test]
    fn compose_checks_four_cases() {
        // 初期 + free外側 → free (AI 全自律)。
        assert_eq!(
            compose(
                autonomy_enforcement(Autonomy::Free),
                phase_enforcement(Phase::Initial)
            ),
            Enforcement::Free
        );
        // 初期 + guarded核 → human-gate (早期でも核は人間)。
        assert_eq!(
            compose(
                autonomy_enforcement(Autonomy::Guarded),
                phase_enforcement(Phase::Initial)
            ),
            Enforcement::PreActionHumanGate
        );
        // 保守 + free外側 → commit-block (phase が勝つ)。
        assert_eq!(
            compose(
                autonomy_enforcement(Autonomy::Free),
                phase_enforcement(Phase::Maintenance)
            ),
            Enforcement::CommitBlock
        );
        // 保守 + guarded核 → human-gate (層が勝つ)。
        assert_eq!(
            compose(
                autonomy_enforcement(Autonomy::Guarded),
                phase_enforcement(Phase::Maintenance)
            ),
            Enforcement::PreActionHumanGate
        );
    }

    #[test]
    fn commit_blocks_guarded_regardless_of_phase() {
        assert!(commit_blocks(Autonomy::Guarded, Phase::Initial));
        assert!(commit_blocks(Autonomy::Guarded, Phase::Maintenance));
    }

    #[test]
    fn commit_blocks_supervised_and_free_only_in_maintenance() {
        assert!(!commit_blocks(Autonomy::Supervised, Phase::Initial));
        assert!(!commit_blocks(Autonomy::Free, Phase::Initial));
        assert!(!commit_blocks(Autonomy::Supervised, Phase::Stable));
        assert!(commit_blocks(Autonomy::Supervised, Phase::Maintenance));
        // free + 保守 = commit-block (検算の「保守+free→commit-block」)。
        assert!(commit_blocks(Autonomy::Free, Phase::Maintenance));
    }
}
