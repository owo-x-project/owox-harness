//! 頻出手順の検知 (`docs/decisions/20260616-Phase8-パターンからスキル育成.md`)。
//!
//! 使用履歴 (`usage.rs`) を読み、隣接する name の列 (長さ 2..=max_len) の出現回数を数える。
//! 閾値超え かつ 未スキル化 の列を「育てられる手順」として助言で返す (advisory・commit を止めない)。
//! 雛形は自動生成しない。提案 = 始端で、技の固定化は既存ライフサイクル (テスト合格 + 人間昇格) が担う。
//!
//! decay と同じく run_* 別関数で既存署名を変えない。読みは usage.log のみ (床へ注入しない)。

use std::collections::BTreeMap;
use std::path::Path;

use crate::quality::RoutineConfig;
use crate::skill::Skill;
use crate::usage;

/// 育てられる手順の提案 1 件。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoutineSuggestion {
    /// 隣接して繰り返される name の列 (入口コマンド / シェル操作)。
    pub sequence: Vec<String>,
    /// 出現回数。
    pub occurrences: usize,
}

/// 使用履歴から頻出手順を検知する。閾値超え かつ 未スキル化 の列を返す。
///
/// 長い列を優先し、既に採られた長い列に含まれる短い部分列は落とす (重複提案を減らす)。
pub fn run_routine_suggestions(
    owox_dir: &Path,
    config: &RoutineConfig,
    skills: &[Skill],
) -> Vec<RoutineSuggestion> {
    let names = usage::read_names(owox_dir);
    if names.len() < 2 || config.max_len < 2 {
        return Vec::new();
    }

    // 長さごとに隣接 n-gram の出現回数を数える。
    let mut counts: BTreeMap<Vec<String>, usize> = BTreeMap::new();
    let max_len = config.max_len.min(names.len());
    for len in 2..=max_len {
        for window in names.windows(len) {
            *counts.entry(window.to_vec()).or_default() += 1;
        }
    }

    // 閾値超え かつ 2 種以上の name を含む候補を長さ降順・出現回数降順で並べる。
    // 同一 name の連続 (例 Bash の連打) は「手順」でなく雑音なので除く (distinct >= 2)。
    let mut candidates: Vec<RoutineSuggestion> = counts
        .into_iter()
        .filter(|(seq, n)| *n as u32 >= config.min_occurrences && has_distinct_steps(seq))
        .map(|(sequence, occurrences)| RoutineSuggestion {
            sequence,
            occurrences,
        })
        .collect();
    candidates.sort_by(|a, b| {
        b.sequence
            .len()
            .cmp(&a.sequence.len())
            .then(b.occurrences.cmp(&a.occurrences))
            .then(a.sequence.cmp(&b.sequence))
    });

    let mut kept: Vec<RoutineSuggestion> = Vec::new();
    for c in candidates {
        // 既に採られた長い列に含まれる部分列は落とす。
        if kept
            .iter()
            .any(|k| is_subsequence(&c.sequence, &k.sequence))
        {
            continue;
        }
        // 未スキル化のものだけ提案する (既存スキルを再提案しない)。
        if is_skilled(&c.sequence, skills) {
            continue;
        }
        kept.push(c);
    }
    kept
}

/// 手順として意味があるか = 異なる name を 2 種以上含むか。
///
/// 同一 name の連続 (シェル操作の連打など) は技にできる「手順」でないので提案しない。
fn has_distinct_steps(sequence: &[String]) -> bool {
    sequence
        .iter()
        .collect::<std::collections::BTreeSet<_>>()
        .len()
        >= 2
}

/// `needle` が `haystack` の連続部分列か (同一は真)。
fn is_subsequence(needle: &[String], haystack: &[String]) -> bool {
    if needle.len() > haystack.len() {
        return false;
    }
    haystack.windows(needle.len()).any(|w| w == needle)
}

/// この手順が既にスキル化されているか (最小の名前照合)。
///
/// 列を slug 化し、いずれかのスキルの id / name の slug がそれを含めばスキル化済みとみなす。
/// 精緻な対応づけは持たない (`docs/decisions/...` の最小方針)。人間がこの手順用スキルを作り
/// 列名にちなんだ名前を付ければ再提案が止まる、という緩い結びつけ。
fn is_skilled(sequence: &[String], skills: &[Skill]) -> bool {
    let key = slugify_seq(sequence);
    skills
        .iter()
        .any(|s| normalize(&s.id).contains(&key) || normalize(&s.name).contains(&key))
}

/// name の列を `-` 連結の slug へ。各 name の記号は `-` に倒す。
fn slugify_seq(sequence: &[String]) -> String {
    sequence
        .iter()
        .map(|n| normalize(n))
        .collect::<Vec<_>>()
        .join("-")
}

/// 識別子を小文字化し、英数以外を `-` に倒して畳む。
fn normalize(s: &str) -> String {
    let mut out = String::new();
    let mut prev_dash = false;
    for c in s.trim().to_lowercase().chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c);
            prev_dash = false;
        } else if !prev_dash {
            out.push('-');
            prev_dash = true;
        }
    }
    out.trim_matches('-').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn tempdir() -> PathBuf {
        use std::sync::atomic::{AtomicU32, Ordering};
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let pid = std::process::id();
        let dir = std::env::temp_dir().join(format!("owox-routine-test-{pid}-{n}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn cfg() -> RoutineConfig {
        RoutineConfig {
            min_occurrences: 3,
            max_len: 4,
        }
    }

    fn seed(dir: &Path, pairs: &[&str]) {
        for name in pairs {
            usage::record(dir, "20260616", name);
        }
    }

    #[test]
    fn frequent_pair_is_suggested() {
        let dir = tempdir();
        // a,b を 3 回繰り返す (間に c を挟み隣接 a,b が 3 回)。
        seed(&dir, &["a", "b", "c", "a", "b", "c", "a", "b"]);
        let s = run_routine_suggestions(&dir, &cfg(), &[]);
        assert!(
            s.iter()
                .any(|r| r.sequence == vec!["a".to_string(), "b".to_string()] && r.occurrences >= 3)
        );
    }

    #[test]
    fn below_threshold_not_suggested() {
        let dir = tempdir();
        seed(&dir, &["a", "b", "a", "b"]); // a,b は 2 回 (閾値 3 未満)
        assert!(run_routine_suggestions(&dir, &cfg(), &[]).is_empty());
    }

    #[test]
    fn longer_sequence_subsumes_subsequence() {
        let dir = tempdir();
        // a,b,c を 3 回。a,b も 3 回現れるが長い列に含まれるので落ちる。
        seed(&dir, &["a", "b", "c", "a", "b", "c", "a", "b", "c"]);
        let s = run_routine_suggestions(&dir, &cfg(), &[]);
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].sequence, vec!["a", "b", "c"]);
    }

    #[test]
    fn skilled_routine_filtered() {
        use crate::skill::Skill;
        let dir = tempdir();
        seed(&dir, &["build", "lint", "build", "lint", "build", "lint"]);
        let skill = Skill {
            id: "build-lint".to_string(),
            name: "build then lint".to_string(),
            description: "d".to_string(),
            skill_md: String::new(),
            implicit: false,
            promoted: false,
            human_gate: false,
            tests: Vec::new(),
            scripts: Vec::new(),
        };
        // build-lint スキルがあるので build,lint は再提案しない。
        let s = run_routine_suggestions(&dir, &cfg(), &[skill]);
        assert!(s.iter().all(|r| r.sequence != vec!["build", "lint"]));
    }

    #[test]
    fn repeated_single_name_is_not_a_routine() {
        let dir = tempdir();
        // 同じ name の連打は手順でない (distinct < 2)。提案しない。
        seed(&dir, &["Bash", "Bash", "Bash", "Bash", "Bash", "Bash"]);
        assert!(run_routine_suggestions(&dir, &cfg(), &[]).is_empty());
    }

    #[test]
    fn distinct_steps_within_noise_still_surface() {
        let dir = tempdir();
        // Bash 連打に混じる edit→verify の 2 種手順は拾う。
        seed(
            &dir,
            &[
                "Bash", "Bash", "edit", "verify", "Bash", "edit", "verify", "Bash", "edit",
                "verify", "edit", "verify", "edit", "verify",
            ],
        );
        let s = run_routine_suggestions(&dir, &cfg(), &[]);
        // edit→verify を含む手順が出る (長い形へ subsume されることはある)。
        assert!(s.iter().any(|r| {
            r.sequence.contains(&"edit".to_string()) && r.sequence.contains(&"verify".to_string())
        }));
        // どの提案も distinct >= 2 (Bash 単独連打は出ない)。
        assert!(!s.is_empty());
        assert!(s.iter().all(|r| has_distinct_steps(&r.sequence)));
    }

    #[test]
    fn empty_usage_yields_nothing() {
        let dir = tempdir();
        assert!(run_routine_suggestions(&dir, &cfg(), &[]).is_empty());
    }
}
