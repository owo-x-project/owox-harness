//! 腐敗検知の適応度関数 (`docs/decisions/20260614-Phase7-腐敗検知の中核.md`)。
//!
//! タスクと来歴を読み、放置 / 孤立 / 重複 / ゾンビ / done未検証 と古い記憶を機械検出する。
//! quality の禁止パターンと同じく「機械検出して報告、効かせ方は commit で phase 適応」の型に載せる。
//!
//! コード/repo の腐敗 (重複ファイル・委譲検査) は run_code_decay が担う
//! (`docs/decisions/20260614-Phase7-コードrepo腐敗検知.md`)。owox は内容一致の重複だけ直接検出し、
//! 死コード等は `[[decay.checks]]` の検査コマンドへ委譲する。
//!
//! core は git/時計を持たず today を引数で受ける (決定論。`docs/decisions/20260613-Phase4-tool記録層.md`)。
//! 古さは ID 日付と note 日付の最新と today の日数差で測る (git mtime は使わない)。

use std::collections::BTreeMap;
use std::path::Path;

use crate::quality::DecayConfig;
use crate::record::{Decision, DecisionStatus, strip_date_prefix};
use crate::task::{DepKind, Task, TaskLinks, TaskStatus};

/// 腐敗の検出 1 件。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecayFinding {
    /// stale / orphan / duplicate / zombie / unverified-done / stale-open-decision / review-decision。
    pub kind: &'static str,
    /// 対象のタスク / 来歴 ID。
    pub subject: String,
    /// 人間向けの説明。
    pub detail: String,
}

impl DecayFinding {
    /// 1 行サマリ (commit ゲートのメッセージ・封筒に使う。QualityViolation と同型)。
    pub fn summary(&self) -> String {
        format!("{} [{}]: {}", self.subject, self.kind, self.detail)
    }

    /// 構造的腐敗か (done未検証・ゾンビ)。commit ゲートはこれだけ phase 適応で block する。
    /// 放置・孤立・重複・来歴鮮度は age / cleanliness 信号で、commit を止めず助言に留める。
    pub fn is_structural(&self) -> bool {
        matches!(self.kind, "unverified-done" | "zombie")
    }
}

/// 腐敗を検出する。`today` は `YYYYMMDD` (mcp が与える)。閾値は quality.toml の `[decay]`。
///
/// today が読めない時は空 (古さを測れないので作業を妨げない)。
pub fn run_decay(
    tasks: &[Task],
    decisions: &[Decision],
    config: &DecayConfig,
    today: &str,
) -> Vec<DecayFinding> {
    let Some(today_days) = ymd_to_days(today) else {
        return Vec::new();
    };
    let mut findings = Vec::new();
    detect_task_decay(tasks, config, today_days, &mut findings);
    detect_decision_decay(decisions, config, today_days, &mut findings);
    findings
}

/// タスクの腐敗 (放置 / 孤立 / 重複 / ゾンビ / done未検証) を検出する。
fn detect_task_decay(
    tasks: &[Task],
    config: &DecayConfig,
    today_days: i64,
    out: &mut Vec<DecayFinding>,
) {
    let stale = config.stale_task_days as i64;

    // 重複: 日付前置を剥がした slug が一致する別タスク (dropped は除く)。
    let mut by_slug: std::collections::BTreeMap<&str, Vec<&str>> =
        std::collections::BTreeMap::new();
    for t in tasks.iter().filter(|t| t.status != TaskStatus::Dropped) {
        by_slug
            .entry(strip_date_prefix(&t.id))
            .or_default()
            .push(&t.id);
    }

    for task in tasks {
        let age = task_age(task, today_days);

        // 放置: 未完のタスクが最後の活動から閾値超え。
        if matches!(
            task.status,
            TaskStatus::Todo | TaskStatus::Doing | TaskStatus::Blocked
        ) && age.is_some_and(|a| a > stale)
        {
            out.push(DecayFinding {
                kind: "stale",
                subject: task.id.clone(),
                detail: format!(
                    "no activity for {} days (status {})",
                    age.unwrap_or(0),
                    status_str(task.status)
                ),
            });
        }

        // 孤立: 未完のタスクが link も dep も無く、かつ古い (新規 todo の誤検出を age ゲートで避ける)。
        // done / dropped は終了済みで繋ぐ義務が無いため対象外 (放置と同じ active 集合に揃える)。
        if matches!(
            task.status,
            TaskStatus::Todo | TaskStatus::Doing | TaskStatus::Blocked
        ) && task.links == TaskLinks::default()
            && task.deps.is_empty()
            && age.is_some_and(|a| a > stale)
        {
            out.push(DecayFinding {
                kind: "orphan",
                subject: task.id.clone(),
                detail: "no links and no dependencies, and stale".to_string(),
            });
        }

        // 重複: 同 slug の別タスクがある (dropped は対象外)。
        if task.status != TaskStatus::Dropped
            && let Some(ids) = by_slug.get(strip_date_prefix(&task.id))
        {
            let others: Vec<&str> = ids
                .iter()
                .copied()
                .filter(|id| *id != task.id.as_str())
                .collect();
            if !others.is_empty() {
                out.push(DecayFinding {
                    kind: "duplicate",
                    subject: task.id.clone(),
                    detail: format!("shares its slug with: {}", others.join(", ")),
                });
            }
        }

        // ゾンビ: blocks 依存が dropped で永久に done になれない、または blocks に循環。
        if matches!(
            task.status,
            TaskStatus::Todo | TaskStatus::Doing | TaskStatus::Blocked
        ) && let Some(reason) = permanently_blocked(task, tasks)
        {
            out.push(DecayFinding {
                kind: "zombie",
                subject: task.id.clone(),
                detail: reason,
            });
        }

        // done未検証: done だが検証 link が無い (過去・import タスクを拾う)。
        if task.status == TaskStatus::Done && task.links.verification.is_none() {
            out.push(DecayFinding {
                kind: "unverified-done",
                subject: task.id.clone(),
                detail: "marked done but has no verification link".to_string(),
            });
        }
    }
}

/// 来歴の鮮度 (放置 open・見直し合図) を検出する。
fn detect_decision_decay(
    decisions: &[Decision],
    config: &DecayConfig,
    today_days: i64,
    out: &mut Vec<DecayFinding>,
) {
    for d in decisions {
        let Some(age) = id_date(&d.id).map(|days| today_days - days) else {
            continue;
        };
        match d.status {
            DecisionStatus::Open if age > config.open_decision_days as i64 => {
                out.push(DecayFinding {
                    kind: "stale-open-decision",
                    subject: d.id.clone(),
                    detail: format!("open and awaiting judgment for {age} days"),
                });
            }
            DecisionStatus::Adopted if age > config.review_decision_days as i64 => {
                out.push(DecayFinding {
                    kind: "review-decision",
                    subject: d.id.clone(),
                    detail: format!("adopted {age} days ago; consider reviewing it"),
                });
            }
            _ => {}
        }
    }
}

/// 成長層 (practices) の鮮度を検出する (`docs/decisions/20260614-Phase7-経験IOと二層ルール.md`)。
///
/// 古い指針を見直し合図として報告する (削除はしない・捨てる根拠にしない)。閾値は来歴の見直しと同じ
/// review_decision_days を使う。run_decay の署名を変えないため別関数 (run_code_decay と同じ流儀)。
/// is_structural()=false なので commit は止めない (鮮度は助言)。
pub fn run_practice_decay(
    practices: &[crate::model::Practice],
    review_days: u32,
    today: &str,
) -> Vec<DecayFinding> {
    let Some(today_days) = ymd_to_days(today) else {
        return Vec::new();
    };
    let mut findings = Vec::new();
    for p in practices {
        let Some(age) = ymd_to_days(&p.date).map(|days| today_days - days) else {
            continue;
        };
        if age > review_days as i64 {
            findings.push(DecayFinding {
                kind: "stale-practice",
                subject: format!("practice {}", p.date),
                detail: format!("grown {age} days ago; consider reviewing it"),
            });
        }
    }
    findings
}

/// 成長層 (practices) の冗長性を検出する (`docs/decisions/20260617-practices冗長性の機械シグナル.md`)。
///
/// practice 対を字 n-gram (3-gram) の Jaccard 類似度で比べ、閾値超えを kind="redundant-practice"
/// として報告する。床コンテキストへ全件注入される practices が似た重複で膨らむのを advisory で気づかせ、
/// 統合は canon.propose (人間ゲート) へ流す。字 n-gram は言語非依存・決定論 (LLM 不要)。
/// run_decay の署名は不変 (別関数)・is_structural()=false (commit を止めない)。
pub fn run_practice_redundancy(
    practices: &[crate::model::Practice],
    min_similarity: f64,
) -> Vec<DecayFinding> {
    let grams: Vec<std::collections::BTreeSet<String>> =
        practices.iter().map(|p| char_ngrams(&p.text, 3)).collect();
    let mut findings = Vec::new();
    for i in 0..practices.len() {
        for j in (i + 1)..practices.len() {
            let sim = jaccard(&grams[i], &grams[j]);
            if sim >= min_similarity {
                findings.push(DecayFinding {
                    kind: "redundant-practice",
                    subject: format!("practice {}", practices[j].date),
                    detail: format!(
                        "looks {}% similar to practice {}; consider merging via canon.propose",
                        (sim * 100.0).round() as u32,
                        practices[i].date
                    ),
                });
            }
        }
    }
    findings
}

/// rules の冗長性を検出する。
///
/// rules の全フィールドを平坦化した String スライスに対して、practice と同じ字 n-gram (3-gram)
/// Jaccard 類似度を使い、閾値超えを kind="duplicate-rule" として報告する。
/// 新たな類似アルゴリズムは発明せず、run_practice_redundancy と同じ char_ngrams / jaccard を流用。
/// is_structural()=false なので commit を止めない (advisory)。
pub fn run_rules_redundancy(rules: &crate::model::Rules, min_similarity: f64) -> Vec<DecayFinding> {
    // 全フィールドを平坦化して (テキスト, セクション名) ペアにする。
    // irreversible / human_gate は構造が異なるため対象外。
    let entries: Vec<(&str, &str)> = rules
        .common
        .iter()
        .map(|s| (s.as_str(), "common"))
        .chain(rules.initial.iter().map(|s| (s.as_str(), "initial")))
        .chain(rules.stable.iter().map(|s| (s.as_str(), "stable")))
        .chain(
            rules
                .maintenance
                .iter()
                .map(|s| (s.as_str(), "maintenance")),
        )
        .chain(
            rules
                .change_policy
                .iter()
                .map(|s| (s.as_str(), "change_policy")),
        )
        .chain(
            rules
                .dependency_policy
                .iter()
                .map(|s| (s.as_str(), "dependency_policy")),
        )
        .chain(
            rules
                .deletion_policy
                .iter()
                .map(|s| (s.as_str(), "deletion_policy")),
        )
        .chain(rules.safety.iter().map(|s| (s.as_str(), "safety")))
        .collect();

    let grams: Vec<std::collections::BTreeSet<String>> = entries
        .iter()
        .map(|(text, _)| char_ngrams(text, 3))
        .collect();
    let mut findings = Vec::new();
    for i in 0..entries.len() {
        for j in (i + 1)..entries.len() {
            let sim = jaccard(&grams[i], &grams[j]);
            if sim >= min_similarity {
                findings.push(DecayFinding {
                    kind: "duplicate-rule",
                    subject: format!("rule [{}/{}]", entries[j].1, short_rule(entries[j].0)),
                    detail: format!(
                        "looks {}% similar to rule [{}/{}]; consider merging via canon.propose",
                        (sim * 100.0).round() as u32,
                        entries[i].1,
                        short_rule(entries[i].0)
                    ),
                });
            }
        }
    }
    findings
}

/// rules の subject 用短縮 (先頭 40 文字)。
fn short_rule(text: &str) -> &str {
    if text.len() <= 40 {
        text
    } else {
        // UTF-8 境界に合わせる。
        let mut end = 40;
        while end > 0 && !text.is_char_boundary(end) {
            end -= 1;
        }
        &text[..end]
    }
}

/// テキストの字 n-gram 集合。短すぎる時は全体を 1 要素にする (言語非依存)。
fn char_ngrams(text: &str, n: usize) -> std::collections::BTreeSet<String> {
    let chars: Vec<char> = text.trim().chars().collect();
    let mut set = std::collections::BTreeSet::new();
    if chars.len() < n {
        if !chars.is_empty() {
            set.insert(chars.iter().collect());
        }
        return set;
    }
    for w in chars.windows(n) {
        set.insert(w.iter().collect());
    }
    set
}

/// 2 集合の Jaccard 類似度 (共通 / 和集合)。どちらも空なら 0。
fn jaccard(a: &std::collections::BTreeSet<String>, b: &std::collections::BTreeSet<String>) -> f64 {
    if a.is_empty() && b.is_empty() {
        return 0.0;
    }
    let inter = a.intersection(b).count();
    let union = a.union(b).count();
    inter as f64 / union as f64
}

/// 調査知識層の鮮度を検出する (`docs/decisions/20260616-Phase8-調査知識層.md`)。
///
/// current の調査が調査日から stale_days を超えたら kind="stale-knowledge" を報告する
/// (superseded は対象外・現役の調査だけ見る)。run_practice_decay と同流儀・run_decay の署名は不変。
/// is_structural()=false なので commit は止めない (鮮度は助言)。
pub fn run_knowledge_decay(
    knowledge: &[crate::knowledge::Knowledge],
    stale_days: u32,
    today: &str,
) -> Vec<DecayFinding> {
    let Some(today_days) = ymd_to_days(today) else {
        return Vec::new();
    };
    let mut findings = Vec::new();
    for k in knowledge {
        if k.status != crate::knowledge::KnowledgeStatus::Current {
            continue;
        }
        let Some(age) = ymd_to_days(&k.researched_on).map(|days| today_days - days) else {
            continue;
        };
        if age > stale_days as i64 {
            findings.push(DecayFinding {
                kind: "stale-knowledge",
                subject: k.id.clone(),
                detail: format!("researched {age} days ago; consider re-checking it"),
            });
        }
    }
    findings
}

/// ブランチ作業記憶の腐敗を検出する (`docs/decisions/20260618-Phase9-ブランチ作業記憶層.md`)。
///
/// 孤児 (ブランチがもう存在しない) と放置 (最新メモが古い) を stale-branch-memory として報告する。
/// 自動削除はせず剪定候補に出すだけ (恒久判断は decisions/・経験は experience/ へ明示昇格)。
/// すべて advisory で commit を止めない。`existing_branches` が空 (git を読めない) 時は孤児判定を飛ばす
/// (全件を誤って孤児にしない・安全側)。
pub fn run_branch_memory_decay(
    memories: &[crate::branch_memory::BranchMemory],
    existing_branches: &[String],
    stale_days: u32,
    today: &str,
) -> Vec<DecayFinding> {
    let today_days = ymd_to_days(today);
    let check_orphan = !existing_branches.is_empty();
    let mut findings = Vec::new();
    for m in memories {
        if check_orphan && !existing_branches.iter().any(|b| b == &m.branch) {
            findings.push(DecayFinding {
                kind: "stale-branch-memory",
                subject: m.branch.clone(),
                detail: "branch no longer exists; promote anything durable to decisions/experience, then prune".to_string(),
            });
            continue;
        }
        if let (Some(td), Some(last)) = (today_days, m.last_date().and_then(|d| ymd_to_days(&d)))
            && td - last > stale_days as i64
        {
            findings.push(DecayFinding {
                kind: "stale-branch-memory",
                subject: m.branch.clone(),
                detail: format!("no notes for {} days", td - last),
            });
        }
    }
    findings
}

/// 検証設定の陳腐化を検出する。
///
/// 各検査が `evidence_paths` に宣言したファイル (work_dir 相対) が実在しない場合、
/// kind="stale-verify-link" として報告する。is_structural()=false なので commit を止めない (advisory)。
/// `evidence_paths` が空の検査はスキップする (宣言なし = 陳腐化判定なし)。決定論的 (与えた順)。
pub fn detect_stale_verify_links(
    checks: &[crate::model::VerifyCheck],
    work_dir: &Path,
) -> Vec<DecayFinding> {
    let mut findings = Vec::new();
    for check in checks {
        for path in &check.evidence_paths {
            if !work_dir.join(path).exists() {
                findings.push(DecayFinding {
                    kind: "stale-verify-link",
                    subject: check.name.clone(),
                    detail: format!(
                        "evidence path {} for check {} no longer exists",
                        path, check.name
                    ),
                });
            }
        }
    }
    findings
}

/// コード/repo の腐敗を検出する (`docs/decisions/20260614-Phase7-コードrepo腐敗検知.md`)。
///
/// owox 直接検出は内容一致の重複ファイルだけ。死コード等は `[[decay.checks]]` の検査コマンドへ委譲する。
/// `files` は work_dir からの相対パス (mcp が列挙して渡す)。重い処理 (全ファイル読取・外部コマンド) を
/// 含むため verify.run でのみ呼ぶ (next には載せない)。すべて advisory で commit を止めない。
pub fn run_code_decay(
    work_dir: &Path,
    files: &[String],
    config: &DecayConfig,
) -> Vec<DecayFinding> {
    let mut findings = detect_duplicate_files(work_dir, files, config.min_duplicate_bytes);
    findings.extend(run_decay_checks(work_dir, &config.checks));
    findings
}

/// 内容が完全一致する追跡ファイルを検出する。
///
/// 内容そのものでグループ化し (ハッシュ衝突誤検出を作らない)、min_bytes 未満や読めないファイルは外す。
/// 2 つ以上一致するグループの各ファイルを duplicate-file として報告する。
fn detect_duplicate_files(
    work_dir: &Path,
    files: &[String],
    min_bytes: usize,
) -> Vec<DecayFinding> {
    let mut by_content: BTreeMap<String, Vec<&String>> = BTreeMap::new();
    for file in files {
        let Ok(content) = std::fs::read_to_string(work_dir.join(file)) else {
            continue; // バイナリ等は飛ばす
        };
        if content.len() < min_bytes {
            continue; // 空・極小は雑音なので外す
        }
        by_content.entry(content).or_default().push(file);
    }

    let mut out = Vec::new();
    for group in by_content.values() {
        if group.len() < 2 {
            continue;
        }
        for file in group {
            let others: Vec<&str> = group
                .iter()
                .filter(|f| **f != *file)
                .map(|f| f.as_str())
                .collect();
            out.push(DecayFinding {
                kind: "duplicate-file",
                subject: (*file).clone(),
                detail: format!("identical content to: {}", others.join(", ")),
            });
        }
    }
    out
}

/// 委譲検査 (`[[decay.checks]]`) を走らせ、非ゼロ終了を decay として報告する。
///
/// 規約: 検査コマンドが decay を見つけたら非ゼロ終了する。run_check を再利用し、通過 (exit 0) = decay 無し、
/// 非通過 = decay 有り。完了を判定する verify.checks と別物 (助言なのでゲートを汚さない)。
fn run_decay_checks(work_dir: &Path, checks: &[crate::model::VerifyCheck]) -> Vec<DecayFinding> {
    crate::verify::run_checks(work_dir, checks)
        .into_iter()
        .filter(|r| !r.passed)
        .map(|r| DecayFinding {
            kind: "decay-check",
            subject: r.name,
            detail: r.detail,
        })
        .collect()
}

/// タスクが永久に done になれないか (blocks の推移閉包に dropped か循環があるか) を調べる。
///
/// 自身から blocks 辺をたどり、対象が dropped → 永久ブロック、経路に再訪 → 循環。
/// 対象が見つからない辺は飛ばす (create が前方参照を弾くため通常起きない)。
fn permanently_blocked(start: &Task, tasks: &[Task]) -> Option<String> {
    let find = |id: &str| tasks.iter().find(|t| t.id == id);
    let mut path: std::collections::BTreeSet<&str> = std::collections::BTreeSet::new();
    walk_blocks(&start.id, &find, &mut path)
}

/// `current` の blocks 先を DFS でたどる。dropped か循環を見つけたら理由を返す。
/// path は現在の探索経路 (循環検出用)。
fn walk_blocks<'a>(
    current: &'a str,
    find: &impl Fn(&str) -> Option<&'a Task>,
    path: &mut std::collections::BTreeSet<&'a str>,
) -> Option<String> {
    let task = find(current)?;
    if !path.insert(current) {
        return Some("circular blocks dependency (can never become ready)".to_string());
    }
    for dep in task.deps.iter().filter(|d| d.kind == DepKind::Blocks) {
        match find(&dep.target) {
            Some(t) if t.status == TaskStatus::Dropped => {
                return Some(format!(
                    "blocked by dropped task {} (can never become done)",
                    dep.target
                ));
            }
            Some(_) => {
                if let Some(reason) = walk_blocks(&dep.target, find, path) {
                    return Some(reason);
                }
            }
            None => {}
        }
    }
    path.remove(current);
    None
}

/// タスクの最後の活動からの日数。ID 日付と note 日付の最新を最後の活動とする。
/// ID 日付が読めなければ None (古さを測れない)。
fn task_age(task: &Task, today_days: i64) -> Option<i64> {
    let mut last = id_date(&task.id)?;
    for note in &task.notes {
        if let Some(days) = note_date(note) {
            last = last.max(days);
        }
    }
    Some(today_days - last)
}

/// ID の先頭 `YYYYMMDD` を通日へ。8 桁の数字でなければ None。
fn id_date(id: &str) -> Option<i64> {
    ymd_to_days(id.get(..8)?)
}

/// note (`YYYYMMDD: 本文`) の先頭日付を通日へ。
fn note_date(note: &str) -> Option<i64> {
    ymd_to_days(note.get(..8)?)
}

/// `YYYYMMDD` を通日 (epoch からの日数) へ。8 桁の数字でなければ None。
fn ymd_to_days(s: &str) -> Option<i64> {
    if s.len() != 8 || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let y: i64 = s[0..4].parse().ok()?;
    let m: u32 = s[4..6].parse().ok()?;
    let d: u32 = s[6..8].parse().ok()?;
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    Some(days_from_civil(y, m, d))
}

/// 西暦年月日を epoch からの日数へ (Howard Hinnant の days_from_civil。civil_from_days の逆)。
fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400; // [0, 399]
    let m = m as i64;
    let d = d as i64;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1; // [0, 365]
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy; // [0, 146096]
    era * 146_097 + doe - 719_468
}

fn status_str(status: TaskStatus) -> &'static str {
    match status {
        TaskStatus::Todo => "todo",
        TaskStatus::Doing => "doing",
        TaskStatus::Done => "done",
        TaskStatus::Blocked => "blocked",
        TaskStatus::Dropped => "dropped",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record::DecisionLinks;
    use crate::task::{Dep, TaskLinks};

    fn task(id: &str, status: TaskStatus) -> Task {
        Task {
            id: id.to_string(),
            title: "t".to_string(),
            status,
            links: TaskLinks::default(),
            deps: Vec::new(),
            notes: Vec::new(),
            layer: None,
            stage: None,
            external: Vec::new(),
        }
    }

    fn decision(id: &str, status: DecisionStatus) -> Decision {
        Decision {
            id: id.to_string(),
            title: "d".to_string(),
            status,
            kind: None,
            rationale: String::new(),
            links: DecisionLinks::default(),
            supersedes: Vec::new(),
            proposed_change: None,
            authorizes: Vec::new(),
            consumed: false,
            approval: None,
            auto_approved: false,
            confirmed: false,
        }
    }

    fn cfg() -> DecayConfig {
        DecayConfig {
            stale_task_days: 14,
            open_decision_days: 7,
            review_decision_days: 90,
            ..DecayConfig::default()
        }
    }

    #[test]
    fn days_roundtrip_known_dates() {
        // 2026-06-14 と 2026-06-01 の差は 13 日。
        let a = ymd_to_days("20260601").unwrap();
        let b = ymd_to_days("20260614").unwrap();
        assert_eq!(b - a, 13);
        // 閏年をまたぐ差。
        let c = ymd_to_days("20240228").unwrap();
        let d = ymd_to_days("20240301").unwrap();
        assert_eq!(d - c, 2); // 2024-02-29 が間にある
    }

    #[test]
    fn invalid_date_is_none() {
        assert!(ymd_to_days("2026013").is_none());
        assert!(ymd_to_days("notadate").is_none());
        assert!(ymd_to_days("20261301").is_none()); // 13 月
    }

    #[test]
    fn stale_task_flagged_after_threshold() {
        let tasks = [task("20260101-old", TaskStatus::Todo)];
        let f = run_decay(&tasks, &[], &cfg(), "20260601");
        assert!(f.iter().any(|f| f.kind == "stale"));
    }

    #[test]
    fn fresh_task_not_stale() {
        let tasks = [task("20260601-fresh", TaskStatus::Todo)];
        let f = run_decay(&tasks, &[], &cfg(), "20260605");
        assert!(!f.iter().any(|f| f.kind == "stale"));
    }

    #[test]
    fn note_refreshes_last_activity() {
        // ID は古いが note が新しければ放置でない。
        let mut t = task("20260101-old", TaskStatus::Doing);
        t.notes.push("20260530: still working".to_string());
        let f = run_decay(&[t], &[], &cfg(), "20260601");
        assert!(!f.iter().any(|f| f.kind == "stale"));
    }

    #[test]
    fn orphan_needs_age_and_no_links() {
        // 新規で link 無しでも古くなければ孤立にしない。
        let fresh = run_decay(
            &[task("20260601-x", TaskStatus::Todo)],
            &[],
            &cfg(),
            "20260605",
        );
        assert!(!fresh.iter().any(|f| f.kind == "orphan"));
        // 古くて link 無し → 孤立。
        let old = run_decay(
            &[task("20260101-x", TaskStatus::Todo)],
            &[],
            &cfg(),
            "20260601",
        );
        assert!(old.iter().any(|f| f.kind == "orphan"));
    }

    #[test]
    fn terminal_tasks_not_orphan() {
        // dropped / done は終了済みで link 義務が無く、古くても孤立にしない (誤検出を防ぐ)。
        let dropped = run_decay(
            &[task("20260101-x", TaskStatus::Dropped)],
            &[],
            &cfg(),
            "20260601",
        );
        assert!(!dropped.iter().any(|f| f.kind == "orphan"));
        let done = run_decay(
            &[task("20260101-y", TaskStatus::Done)],
            &[],
            &cfg(),
            "20260601",
        );
        assert!(!done.iter().any(|f| f.kind == "orphan"));
    }

    #[test]
    fn duplicate_slug_flagged() {
        let tasks = [
            task("20260101-write-parser", TaskStatus::Todo),
            task("20260201-write-parser", TaskStatus::Todo),
        ];
        let f = run_decay(&tasks, &[], &cfg(), "20260601");
        let dups: Vec<_> = f.iter().filter(|f| f.kind == "duplicate").collect();
        assert_eq!(dups.len(), 2); // 両方が報告される
    }

    #[test]
    fn dropped_task_not_duplicate() {
        let tasks = [
            task("20260101-x", TaskStatus::Dropped),
            task("20260201-x", TaskStatus::Todo),
        ];
        let f = run_decay(&tasks, &[], &cfg(), "20260601");
        assert!(!f.iter().any(|f| f.kind == "duplicate"));
    }

    #[test]
    fn zombie_when_blocked_by_dropped() {
        let mut dependent = task("20260601-dep", TaskStatus::Todo);
        dependent.deps.push(Dep {
            kind: DepKind::Blocks,
            target: "20260101-gone".to_string(),
        });
        let tasks = [task("20260101-gone", TaskStatus::Dropped), dependent];
        let f = run_decay(&tasks, &[], &cfg(), "20260601");
        assert!(f.iter().any(|f| f.kind == "zombie"));
    }

    #[test]
    fn zombie_on_blocks_cycle() {
        let mut a = task("20260601-a", TaskStatus::Todo);
        a.deps.push(Dep {
            kind: DepKind::Blocks,
            target: "20260601-b".to_string(),
        });
        let mut b = task("20260601-b", TaskStatus::Todo);
        b.deps.push(Dep {
            kind: DepKind::Blocks,
            target: "20260601-a".to_string(),
        });
        let f = run_decay(&[a, b], &[], &cfg(), "20260601");
        assert!(f.iter().filter(|f| f.kind == "zombie").count() >= 1);
    }

    #[test]
    fn normal_blocking_is_not_zombie() {
        // 未 done の前提に blocks されるだけなら正常 (ゾンビでない)。
        let mut dependent = task("20260601-dep", TaskStatus::Todo);
        dependent.deps.push(Dep {
            kind: DepKind::Blocks,
            target: "20260601-pre".to_string(),
        });
        let tasks = [task("20260601-pre", TaskStatus::Todo), dependent];
        let f = run_decay(&tasks, &[], &cfg(), "20260601");
        assert!(!f.iter().any(|f| f.kind == "zombie"));
    }

    #[test]
    fn done_without_verification_flagged() {
        let f = run_decay(
            &[task("20260601-d", TaskStatus::Done)],
            &[],
            &cfg(),
            "20260601",
        );
        assert!(f.iter().any(|f| f.kind == "unverified-done"));
    }

    #[test]
    fn done_with_verification_clean() {
        let mut t = task("20260601-d", TaskStatus::Done);
        t.links.verification = Some("checks passed".to_string());
        let f = run_decay(&[t], &[], &cfg(), "20260601");
        assert!(!f.iter().any(|f| f.kind == "unverified-done"));
    }

    #[test]
    fn stale_open_decision_flagged() {
        let d = [decision("20260101-x", DecisionStatus::Open)];
        let f = run_decay(&[], &d, &cfg(), "20260601");
        assert!(f.iter().any(|f| f.kind == "stale-open-decision"));
    }

    #[test]
    fn old_adopted_decision_review_signal() {
        let d = [decision("20260101-x", DecisionStatus::Adopted)];
        let f = run_decay(&[], &d, &cfg(), "20260601");
        assert!(f.iter().any(|f| f.kind == "review-decision"));
    }

    #[test]
    fn stale_knowledge_flagged_and_superseded_skipped() {
        use crate::knowledge::{Knowledge, KnowledgeStatus};
        let k = |id: &str, on: &str, status| Knowledge {
            id: id.to_string(),
            title: "t".to_string(),
            researched_on: on.to_string(),
            sources: Vec::new(),
            summary: String::new(),
            tags: Vec::new(),
            status,
            supersedes: Vec::new(),
        };
        let items = [
            k("20260101-old", "20260101", KnowledgeStatus::Current),
            k("20260601-fresh", "20260601", KnowledgeStatus::Current),
            // 古いが superseded は対象外。
            k("20260101-gone", "20260101", KnowledgeStatus::Superseded),
        ];
        let f = run_knowledge_decay(&items, 90, "20260601");
        let stale: Vec<_> = f.iter().filter(|f| f.kind == "stale-knowledge").collect();
        assert_eq!(stale.len(), 1);
        assert_eq!(stale[0].subject, "20260101-old");
        // 鮮度は構造的でない (commit を止めない)。
        assert!(!stale[0].is_structural());
    }

    #[test]
    fn redundant_practices_flagged_and_distinct_clean() {
        use crate::model::Practice;
        let p = |date: &str, text: &str| Practice {
            date: date.to_string(),
            text: text.to_string(),
        };
        // ほぼ同旨の 2 件 + 無関係 1 件。
        let practices = [
            p(
                "20260101",
                "OAuth refresh token rotation is optional per RFC 6749; not a hard requirement.",
            ),
            p(
                "20260601",
                "OAuth refresh token rotation is optional per RFC 6749 and not a hard requirement.",
            ),
            p(
                "20260601",
                "Prefer small diffs and run tests before commit.",
            ),
        ];
        let f = run_practice_redundancy(&practices, 0.5);
        let red: Vec<_> = f
            .iter()
            .filter(|f| f.kind == "redundant-practice")
            .collect();
        assert_eq!(red.len(), 1); // 似た 1 対だけ
        assert!(!red[0].is_structural()); // 助言 (commit を止めない)
        assert!(red[0].detail.contains("canon.propose"));
    }

    #[test]
    fn dissimilar_practices_not_flagged() {
        use crate::model::Practice;
        let practices = [
            Practice {
                date: "20260101".to_string(),
                text: "Always write a failing test first.".to_string(),
            },
            Practice {
                date: "20260201".to_string(),
                text: "Document every public function in the API module.".to_string(),
            },
        ];
        assert!(run_practice_redundancy(&practices, 0.5).is_empty());
    }

    #[test]
    fn unmeasurable_today_yields_nothing() {
        let tasks = [task("20260101-old", TaskStatus::Todo)];
        assert!(run_decay(&tasks, &[], &cfg(), "bad").is_empty());
    }

    #[test]
    fn structural_classification() {
        let zombie = DecayFinding {
            kind: "zombie",
            subject: "x".to_string(),
            detail: String::new(),
        };
        let stale = DecayFinding {
            kind: "stale",
            subject: "x".to_string(),
            detail: String::new(),
        };
        assert!(zombie.is_structural());
        assert!(!stale.is_structural());
        // コード decay は advisory (構造的でない)。
        let dup = DecayFinding {
            kind: "duplicate-file",
            subject: "a".to_string(),
            detail: String::new(),
        };
        assert!(!dup.is_structural());
    }

    fn tempdir() -> std::path::PathBuf {
        use std::sync::atomic::{AtomicU32, Ordering};
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let pid = std::process::id();
        let dir = std::env::temp_dir().join(format!("owox-decay-code-test-{pid}-{n}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write(dir: &Path, rel: &str, content: &str) {
        let path = dir.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }

    #[test]
    fn duplicate_files_flagged_both() {
        let dir = tempdir();
        let body = "fn main() { println!(\"hello world, this is long enough\"); }\n";
        write(&dir, "a.rs", body);
        write(&dir, "b.rs", body);
        write(
            &dir,
            "c.rs",
            "fn other() { /* different content entirely here */ }\n",
        );
        let files = vec!["a.rs".to_string(), "b.rs".to_string(), "c.rs".to_string()];
        let cfg = DecayConfig {
            min_duplicate_bytes: 8,
            ..DecayConfig::default()
        };
        let f = run_code_decay(&dir, &files, &cfg);
        let dups: Vec<_> = f.iter().filter(|f| f.kind == "duplicate-file").collect();
        assert_eq!(dups.len(), 2); // a と b の両方
        assert!(
            dups.iter()
                .all(|f| f.subject == "a.rs" || f.subject == "b.rs")
        );
    }

    #[test]
    fn tiny_identical_files_ignored() {
        let dir = tempdir();
        write(&dir, "x.rs", "\n");
        write(&dir, "y.rs", "\n");
        let files = vec!["x.rs".to_string(), "y.rs".to_string()];
        // min_duplicate_bytes 既定 (64) 未満なので雑音として外す。
        let f = run_code_decay(&dir, &files, &DecayConfig::default());
        assert!(!f.iter().any(|f| f.kind == "duplicate-file"));
    }

    #[test]
    fn decay_check_nonzero_exit_is_reported() {
        let dir = tempdir();
        let cfg = DecayConfig {
            checks: vec![
                crate::model::VerifyCheck {
                    name: "dead code".to_string(),
                    command: "exit 1".to_string(), // decay 有り (非ゼロ)
                    evidence_paths: Vec::new(),
                },
                crate::model::VerifyCheck {
                    name: "clean".to_string(),
                    command: "true".to_string(), // decay 無し (通過)
                    evidence_paths: Vec::new(),
                },
            ],
            ..DecayConfig::default()
        };
        let f = run_code_decay(&dir, &[], &cfg);
        let checks: Vec<_> = f.iter().filter(|f| f.kind == "decay-check").collect();
        assert_eq!(checks.len(), 1);
        assert_eq!(checks[0].subject, "dead code");
    }

    #[test]
    fn branch_memory_orphan_and_stale() {
        use crate::branch_memory::BranchMemory;
        let mems = vec![
            BranchMemory {
                branch: "feat/live".to_string(),
                notes: vec!["20260618: recent".to_string()],
            },
            BranchMemory {
                branch: "feat/gone".to_string(),
                notes: vec!["20260618: was here".to_string()],
            },
            BranchMemory {
                branch: "feat/old".to_string(),
                notes: vec!["20260101: long ago".to_string()],
            },
        ];
        let existing = vec!["feat/live".to_string(), "feat/old".to_string()];
        let f = run_branch_memory_decay(&mems, &existing, 30, "20260618");
        // gone は孤児、old は放置。live は出ない。
        let subjects: Vec<_> = f.iter().map(|x| x.subject.as_str()).collect();
        assert!(subjects.contains(&"feat/gone"));
        assert!(subjects.contains(&"feat/old"));
        assert!(!subjects.contains(&"feat/live"));
        assert!(f.iter().all(|x| x.kind == "stale-branch-memory"));
    }

    #[test]
    fn branch_memory_skips_orphan_when_branches_unknown() {
        use crate::branch_memory::BranchMemory;
        let mems = vec![BranchMemory {
            branch: "feat/x".to_string(),
            notes: vec!["20260618: recent".to_string()],
        }];
        // existing が空 (git を読めない) なら孤児判定を飛ばす (誤検出しない)。
        let f = run_branch_memory_decay(&mems, &[], 30, "20260618");
        assert!(f.is_empty());
    }

    // --- detect_stale_verify_links ---

    fn verify_check(name: &str, paths: Vec<&str>) -> crate::model::VerifyCheck {
        crate::model::VerifyCheck {
            name: name.to_string(),
            command: "true".to_string(),
            evidence_paths: paths.into_iter().map(|p| p.to_string()).collect(),
        }
    }

    #[test]
    fn stale_verify_link_missing_path_flagged() {
        let dir = tempdir();
        // "report.txt" は作成しない → missing。
        let checks = vec![verify_check("ci-report", vec!["report.txt"])];
        let f = detect_stale_verify_links(&checks, &dir);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].kind, "stale-verify-link");
        assert_eq!(f[0].subject, "ci-report");
        assert!(f[0].detail.contains("report.txt"));
        assert!(f[0].detail.contains("ci-report"));
        // 陳腐化リンクは advisory (commit を止めない)。
        assert!(!f[0].is_structural());
    }

    #[test]
    fn stale_verify_link_existing_path_clean() {
        let dir = tempdir();
        write(&dir, "proof.txt", "ok");
        let checks = vec![verify_check("my-check", vec!["proof.txt"])];
        let f = detect_stale_verify_links(&checks, &dir);
        assert!(f.is_empty());
    }

    #[test]
    fn stale_verify_link_empty_evidence_paths_yields_none() {
        let dir = tempdir();
        let checks = vec![verify_check("no-evidence", vec![])];
        let f = detect_stale_verify_links(&checks, &dir);
        assert!(f.is_empty());
    }
}
