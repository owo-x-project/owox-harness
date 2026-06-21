//! 統一 canon 編集モデル (`docs/decisions/20260614-Phase7-経験IOと二層ルール.md` の追補)。
//!
//! brand / rules / practices / glossary を同じ作法で編集する:
//! - add (追加): AI 直接 + 来歴。追加は既存の方向を壊さない低リスク
//! - change / remove (変更・削除): AI 提案 → open gate → needs_human。canon は直接書かない
//!
//! 内部は per-target の実装 (glossary::add・practices::add・brand/rules への追記) を再利用し、
//! 入口だけ統一する。固定 (brand/rules/glossary) と成長 (practices) は内容の差で、編集の作法は同一。

use std::path::Path;

use crate::envelope::{Envelope, Gate};
use crate::record::{
    DecisionLinks, DecisionStatus, ProposedChange, RecordInput, load_decision, record_decision,
    record_decision_with_change,
};

/// canon.add。canon へ項目を 1 件 AI 直接追加し、来歴へ記録する。
///
/// target = brand / rules / practices / glossary。
/// brand / rules は section (どの一覧か) が要る。glossary は text = "用語: 定義"。practices は text = 指針。
pub fn canon_add(
    owox_dir: &Path,
    today: &str,
    target: &str,
    section: Option<&str>,
    text: &str,
) -> Envelope {
    let text = text.trim();
    if text.is_empty() {
        return Envelope::failed("text が空");
    }
    match target.trim() {
        "glossary" => {
            // text = "用語: 定義"。glossary::add (重複検査・Forbidden 保護つき) を再利用する。
            let (term, definition) = crate::markdown::split_pair(text);
            if definition.is_empty() {
                return Envelope::failed("glossary は text を \"用語: 定義\" で渡す");
            }
            crate::glossary::add(owox_dir, today, &term, &definition)
        }
        "practices" => crate::practices::add(owox_dir, today, text),
        "brand" => add_to_prose(
            owox_dir,
            today,
            "brand",
            "brand.md",
            BRAND_SECTIONS,
            section,
            text,
        ),
        "rules" => add_to_prose(
            owox_dir,
            today,
            "rules",
            "rules.md",
            RULES_SECTIONS,
            section,
            text,
        ),
        other => Envelope::failed(format!(
            "canon.add の target は brand / rules / practices / glossary: {other}"
        )),
    }
}

/// canon.propose の入力。変更・削除を提案する (人間ゲート)。canon はこの時点では変えない。
pub struct ProposeInput<'a> {
    /// 対象: brand / rules / practices / glossary。
    pub target: &'a str,
    /// 構造化変更の種類: remove / replace。None なら自由文提案。
    pub op: Option<&'a str>,
    /// brand / rules の見出しキー (どの一覧か)。glossary / practices では不要。
    pub section: Option<&'a str>,
    /// remove / replace で対象にする既存項目のテキスト。
    pub item: Option<&'a str>,
    /// replace の置換後テキスト。
    pub to: Option<&'a str>,
    /// op を使わない自由文の提案 (単純項目でない編集の逃げ道)。
    pub change: Option<&'a str>,
}

/// canon.propose。canon の変更・削除を提案する (人間ゲート)。
///
/// 二通り:
/// - 構造化 (op = remove / replace): owox が対象項目を照合し、open gate に具体変更を保存する。
///   人間が gate.approve で承認すると owox が canon へ適用する (人間は手編集しない)。
/// - 自由文 (change のみ): 単純項目でない編集の逃げ道。canon は変えず、人間が手で編集して承認する。
pub fn canon_propose(owox_dir: &Path, today: &str, input: ProposeInput) -> Envelope {
    let target = input.target.trim();
    let Some(file) = target_file(target) else {
        return Envelope::failed(format!(
            "canon.propose の target は brand / rules / practices / glossary: {target}"
        ));
    };

    if let Some(op) = input.op.map(str::trim).filter(|s| !s.is_empty()) {
        return propose_structured(owox_dir, today, target, file, op, &input);
    }

    let change = input.change.unwrap_or("").trim();
    if change.is_empty() {
        return Envelope::failed(
            "op (remove / replace) で具体的な変更を出すか、change で自由文の提案を出す",
        );
    }

    let rec = record_decision(
        owox_dir,
        today,
        RecordInput {
            title: format!("Propose change to {target}"),
            status: DecisionStatus::Open,
            rationale: format!("Proposed change to {file}. {change}"),
            links: DecisionLinks::default(),
            supersedes: Vec::new(),
        },
    );

    Envelope::needs_human(
        format!(
            "Changing or removing part of {target} is a decision for a human. Recorded an open gate; this free-text proposal needs a human to edit {file} and approve. For a removal or replacement of one item, pass op=remove/replace so owox applies it on approval. To only add, use canon.add."
        ),
        Gate {
            kind: "canon".to_string(),
            subject: format!("{target} change"),
            requires: format!(
                "Edit {file} in the canon for this free-text proposal, then call gate.approve yourself; its CLI confirmation prompt is the human's approval. Or the human rejects it."
            ),
        },
    )
    .with_decision_ids(rec.decision_ids)
}

/// 構造化変更 (remove / replace) を提案する。対象項目を照合し、open gate に保存する。
fn propose_structured(
    owox_dir: &Path,
    today: &str,
    target: &str,
    file: &str,
    op: &str,
    input: &ProposeInput,
) -> Envelope {
    if op != "remove" && op != "replace" {
        return Envelope::failed(format!("op は remove / replace のいずれか: {op}"));
    }
    let Some(item) = input.item.map(str::trim).filter(|s| !s.is_empty()) else {
        return Envelope::failed("op には item (対象にする既存項目のテキスト) が要る");
    };
    let to = if op == "replace" {
        match input.to.map(str::trim).filter(|s| !s.is_empty()) {
            Some(t) => Some(t.to_string()),
            None => return Envelope::failed("op=replace には to (置換後テキスト) が要る"),
        }
    } else {
        None
    };

    let heading = match heading_for(target, input.section) {
        Ok(h) => h,
        Err(err) => return Envelope::failed(err),
    };

    let path = owox_dir.join(file);
    let body = std::fs::read_to_string(&path).unwrap_or_default();
    let items = items_under_heading(&body, &heading);
    let matches = find_matches(&items, target, item);
    if matches.len() != 1 {
        // 編集時だけ対象見出しの項目を返す (直読み禁止は維持・AI が正確に再提案できる)。
        let reason = if matches.is_empty() {
            format!(
                "'{item}' が {target} の {heading} に見つからない。下の items から正確な item を選んで再提案する"
            )
        } else {
            format!(
                "'{item}' が {target} の {heading} に複数一致する。下の items から1件に絞って再提案する"
            )
        };
        return Envelope::failed(reason).with_data(serde_json::json!({
            "target": target,
            "heading": heading,
            "items": items,
        }));
    }
    let matched = matches.into_iter().next().unwrap();

    let action = match &to {
        Some(to) => format!("Replace in {target} {heading}: \"{matched}\" -> \"{to}\""),
        None => format!("Remove from {target} {heading}: \"{matched}\""),
    };
    let rec = record_decision_with_change(
        owox_dir,
        today,
        RecordInput {
            title: format!("Propose change to {target}"),
            status: DecisionStatus::Open,
            rationale: format!("Proposed change to {file}. {action}"),
            links: DecisionLinks::default(),
            supersedes: Vec::new(),
        },
        Some(ProposedChange {
            target: target.to_string(),
            heading,
            op: op.to_string(),
            item: matched,
            to,
        }),
    );

    Envelope::needs_human(
        format!(
            "Changing the canon is a decision for a human. Recorded an open gate with the exact change. When the human decides to approve, call gate.approve yourself; its CLI confirmation prompt is the human's approval, and owox then applies the change to {file}. Do not edit {file} or the generated files yourself, and do not ask the human to call any tool."
        ),
        Gate {
            kind: "canon".to_string(),
            subject: format!("{target} change"),
            requires: format!(
                "When the human decides, call gate.approve yourself; its CLI confirmation prompt is the human's approval and owox then applies the change to {file}. Or the human rejects it."
            ),
        },
    )
    .with_decision_ids(rec.decision_ids)
}

/// gate.approve 時に呼ぶ。来歴に紐づく canon 変更があれば canon へ適用する。
///
/// 変更が無い来歴 (通常のゲート) は何もしない (Ok)。status=open でない時も何もしない
/// (承認可否は呼び出し側の approve_gate が判定する)。適用に失敗したら Err を返し承認させない。
pub fn apply_pending_canon_change(owox_dir: &Path, id: &str) -> Result<(), String> {
    let decision = load_decision(owox_dir, id)?;
    if decision.status != DecisionStatus::Open {
        return Ok(());
    }
    let Some(change) = decision.proposed_change else {
        return Ok(());
    };
    let Some(file) = target_file(&change.target) else {
        return Err(format!("未知の canon target: {}", change.target));
    };
    let path = owox_dir.join(file);
    let body = std::fs::read_to_string(&path)
        .map_err(|e| format!("{} を読めない: {e}", path.display()))?;

    let updated = match change.op.as_str() {
        // add は追記 (訂正からの practice 草案など)。見出しが無ければ末尾に新設する。
        "add" => Some(append_under_heading(&body, &change.heading, &change.item)),
        "remove" => edit_item_under_heading(&body, &change.heading, &change.item, None),
        "replace" => edit_item_under_heading(
            &body,
            &change.heading,
            &change.item,
            Some(change.to.as_deref().unwrap_or_default()),
        ),
        other => return Err(format!("未知の op: {other}")),
    };
    let Some(updated) = updated else {
        return Err(format!(
            "対象項目が {} の {} に見つからない (canon が変わった可能性): {}",
            change.target, change.heading, change.item
        ));
    };
    std::fs::write(&path, updated).map_err(|e| format!("{} へ書けない: {e}", path.display()))?;
    Ok(())
}

/// gate.revert 時に呼ぶ。adopted な来歴に紐づく canon 変更を逆適用し、canon を元へ戻す。
///
/// 変更が無い来歴は何もしない (Ok)。逆操作: add↔remove、replace は to↔item を入れ替える。
/// 自動承認の差し戻し専用。逆適用に失敗したら Err を返し差し戻させない。
pub fn revert_pending_canon_change(owox_dir: &Path, id: &str) -> Result<(), String> {
    let decision = load_decision(owox_dir, id)?;
    let Some(change) = decision.proposed_change else {
        return Ok(());
    };
    let Some(file) = target_file(&change.target) else {
        return Err(format!("未知の canon target: {}", change.target));
    };
    let path = owox_dir.join(file);
    let body = std::fs::read_to_string(&path)
        .map_err(|e| format!("{} を読めない: {e}", path.display()))?;

    let updated = match change.op.as_str() {
        // add の逆 = 追記した項目を消す。
        "add" => edit_item_under_heading(&body, &change.heading, &change.item, None),
        // remove の逆 = 消した項目を戻す。
        "remove" => Some(append_under_heading(&body, &change.heading, &change.item)),
        // replace の逆 = 置換後 (to) を元 (item) へ戻す。
        "replace" => {
            let to = change.to.as_deref().unwrap_or_default();
            edit_item_under_heading(&body, &change.heading, to, Some(&change.item))
        }
        other => return Err(format!("未知の op: {other}")),
    };
    let Some(updated) = updated else {
        return Err(format!(
            "逆適用の対象項目が {} の {} に見つからない (canon が変わった可能性)",
            change.target, change.heading
        ));
    };
    std::fs::write(&path, updated).map_err(|e| format!("{} へ書けない: {e}", path.display()))?;
    Ok(())
}

/// correction.note。人間が AI を訂正した事実から、成長層 (practices) への追加を open gate として起草する。
///
/// 固定はしない (人間承認で初めて practices へ載る)。承認 = practices への追記 (op=add)。
/// auto 窓が開いていれば gate.auto_approve で自動承認でき、後追いキューへ積まれる
/// (`docs/decisions/20260619-承認と自動改善ループ.md`)。
pub fn propose_practice_from_correction(
    owox_dir: &Path,
    today: &str,
    summary: &str,
    lesson: &str,
) -> Envelope {
    let lesson = lesson.trim();
    if lesson.is_empty() {
        return Envelope::failed("lesson が空。次から守る指針を 1 文で渡す");
    }
    let summary = summary.trim();
    let rationale = if summary.is_empty() {
        format!("Drafted from a human correction. Proposed practice: {lesson}")
    } else {
        format!("Drafted from a human correction: {summary}. Proposed practice: {lesson}")
    };
    let rec = record_decision_with_change(
        owox_dir,
        today,
        RecordInput {
            title: format!("Proposed practice from correction: {lesson}"),
            status: DecisionStatus::Open,
            rationale,
            links: DecisionLinks::default(),
            supersedes: Vec::new(),
        },
        Some(ProposedChange {
            target: "practices".to_string(),
            heading: "Practices".to_string(),
            op: "add".to_string(),
            item: lesson.to_string(),
            to: None,
        }),
    );
    Envelope::needs_human(
        "Drafted a practice from the correction and recorded it as an open gate. It is not fixed yet. When the human decides to approve, call gate.approve yourself (its CLI confirmation prompt is the human's approval) and owox adds it to the practices; or the human rejects it. If automatic approval is on for this session, you may approve it with gate.auto_approve instead.".to_string(),
        Gate {
            kind: "practice-draft".to_string(),
            subject: "practices add from correction".to_string(),
            requires:
                "A human approves the gate (you call gate.approve or gate.auto_approve), then owox adds the practice. Or the human rejects it."
                    .to_string(),
        },
    )
    .with_decision_ids(rec.decision_ids)
}

/// target → 正本ファイル名。
fn target_file(target: &str) -> Option<&'static str> {
    match target {
        "brand" => Some("brand.md"),
        "rules" => Some("rules.md"),
        "practices" => Some("practices.md"),
        "glossary" => Some("glossary.md"),
        _ => None,
    }
}

/// target (+ section) → 対象見出し。glossary / practices は固定、brand / rules は section から。
fn heading_for(target: &str, section: Option<&str>) -> Result<String, String> {
    match target {
        "glossary" => Ok("Terms".to_string()),
        "practices" => Ok("Practices".to_string()),
        "brand" => section_heading(target, BRAND_SECTIONS, section),
        "rules" => section_heading(target, RULES_SECTIONS, section),
        other => Err(format!("未知の canon target: {other}")),
    }
}

fn section_heading(
    target: &str,
    sections: &[(&str, &str)],
    section: Option<&str>,
) -> Result<String, String> {
    let Some(section) = section else {
        return Err(format!(
            "{target} の変更は section が要る ({})",
            section_keys(sections)
        ));
    };
    let key = normalize_section(section);
    sections
        .iter()
        .find(|(k, _)| *k == key)
        .map(|(_, h)| h.to_string())
        .ok_or_else(|| {
            format!(
                "{target} の section は {} のいずれか: {section}",
                section_keys(sections)
            )
        })
}

/// `## 見出し` 配下の `- ` 項目のテキスト一覧を返す。
fn items_under_heading(body: &str, heading: &str) -> Vec<String> {
    let needle = format!("## {heading}");
    let mut in_section = false;
    let mut items = Vec::new();
    for line in body.lines() {
        let t = line.trim_end();
        if t == needle {
            in_section = true;
            continue;
        }
        if in_section && t.starts_with("## ") {
            break;
        }
        if in_section && let Some(rest) = line.trim_start().strip_prefix("- ") {
            items.push(rest.trim().to_string());
        }
    }
    items
}

/// 提案の item に一致する既存項目を返す。完全一致を優先し、glossary は用語名でも引ける。
fn find_matches(items: &[String], target: &str, item: &str) -> Vec<String> {
    let item = item.trim();
    let exact: Vec<String> = items.iter().filter(|i| i.trim() == item).cloned().collect();
    if !exact.is_empty() {
        return exact;
    }
    if target == "glossary" {
        // 用語名は `用語 | 別名 | 別名` の形をとりうる。正規名・別名のどれかが item と一致すれば拾う。
        return items
            .iter()
            .filter(|i| {
                let names = crate::markdown::split_pair(i).0;
                names.split('|').map(str::trim).any(|n| n == item)
            })
            .cloned()
            .collect();
    }
    Vec::new()
}

/// `## 見出し` 配下で item に一致する `- ` 行を削除 (replacement=None) または置換する。
/// 一致が無ければ None。一致は最初の 1 件だけに適用する (propose で一意を保証済み)。
fn edit_item_under_heading(
    body: &str,
    heading: &str,
    item: &str,
    replacement: Option<&str>,
) -> Option<String> {
    let needle = format!("## {heading}");
    let target = item.trim();
    let mut in_section = false;
    let mut done = false;
    let mut out = String::new();
    for line in body.split_inclusive('\n') {
        let t = line.trim_end();
        if t == needle {
            in_section = true;
            out.push_str(line);
            continue;
        }
        if in_section && t.starts_with("## ") {
            in_section = false;
        }
        if in_section
            && !done
            && let Some(rest) = t.trim_start().strip_prefix("- ")
            && rest.trim() == target
        {
            done = true;
            match replacement {
                None => continue,
                Some(to) => {
                    out.push_str(&format!("- {}\n", to.trim()));
                    continue;
                }
            }
        }
        out.push_str(line);
    }
    done.then_some(out)
}

/// brand.md / rules.md の見出し配下へ `- text` を追記し、来歴へ記録する。
fn add_to_prose(
    owox_dir: &Path,
    today: &str,
    target: &str,
    file: &str,
    sections: &[(&str, &str)],
    section: Option<&str>,
    text: &str,
) -> Envelope {
    // 有効な section の一覧 (不一致フィードバックで返す)。
    let valid: Vec<&str> = sections.iter().map(|(k, _)| *k).collect();
    let Some(section) = section else {
        return Envelope::failed(format!(
            "{target} の add は section が要る ({})",
            section_keys(sections)
        ))
        .with_data(serde_json::json!({ "target": target, "valid_sections": valid }));
    };
    let key = normalize_section(section);
    let Some((_, heading)) = sections.iter().find(|(k, _)| *k == key) else {
        return Envelope::failed(format!(
            "{target} の section は {} のいずれか: {section}",
            section_keys(sections)
        ))
        .with_data(serde_json::json!({ "target": target, "valid_sections": valid }));
    };

    let path = owox_dir.join(file);
    let body = std::fs::read_to_string(&path).unwrap_or_default();
    let updated = append_under_heading(&body, heading, text);
    if let Err(err) = std::fs::write(&path, updated) {
        return Envelope::failed(format!("{} へ書けない: {err}", path.display()));
    }

    let rec = record_decision(
        owox_dir,
        today,
        RecordInput {
            title: format!("Add to {target} {heading}"),
            status: DecisionStatus::Adopted,
            rationale: format!("Added a {target} item under {heading}. {text}"),
            links: DecisionLinks::default(),
            supersedes: Vec::new(),
        },
    );
    Envelope::ok(
        format!("Added an item under {target} {heading}."),
        serde_json::json!({ "target": target, "section": heading, "text": text }),
    )
    .with_decision_ids(rec.decision_ids)
}

/// `## 見出し` 配下へ `- text` を追記する。見出しが無ければ末尾へ新設する。
fn append_under_heading(body: &str, heading: &str, text: &str) -> String {
    let needle = format!("## {heading}");
    let item = format!("- {text}\n");
    let mut out = String::new();
    let mut inserted = false;
    for line in body.split_inclusive('\n') {
        out.push_str(line);
        if !inserted && line.trim_end() == needle {
            // 見出し直後へ差し込む (節内の並びは些末)。
            out.push_str(&item);
            inserted = true;
        }
    }
    if inserted {
        return out;
    }
    // 見出しが無い: 末尾へ新設する。
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    out.push_str(&format!("\n## {heading}\n\n{item}"));
    out
}

/// brand.md の追記可能な見出し (key → 見出し)。
const BRAND_SECTIONS: &[(&str, &str)] = &[
    ("values", "Values"),
    ("principles", "Principles"),
    ("non-goals", "Non-goals"),
    ("success-criteria", "Success criteria"),
    ("style", "Style"),
];

/// rules.md の追記可能な見出し。irreversible / human_gate は機械強制・構造化のため add 対象外
/// (変更は canon.propose 経由 = 安全側)。
const RULES_SECTIONS: &[(&str, &str)] = &[
    ("change-policy", "Change policy"),
    ("dependency-policy", "Dependency policy"),
    ("deletion-policy", "Deletion policy"),
    ("safety", "Safety"),
];

/// section 入力を正規化する (小文字・空白/下線を `-` へ)。
fn normalize_section(section: &str) -> String {
    section.trim().to_lowercase().replace([' ', '_'], "-")
}

fn section_keys(sections: &[(&str, &str)]) -> String {
    sections
        .iter()
        .map(|(k, _)| *k)
        .collect::<Vec<_>>()
        .join(" / ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::envelope::Status;
    use crate::model::{Brand, Practices};
    use std::path::PathBuf;

    fn tempdir() -> PathBuf {
        use std::sync::atomic::{AtomicU32, Ordering};
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let pid = std::process::id();
        let dir = std::env::temp_dir().join(format!("owox-canon-test-{pid}-{n}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn add_brand_value_appends_and_records() {
        let owox = tempdir();
        std::fs::write(
            owox.join("brand.md"),
            "## Vision\n\nShip it.\n\n## Values\n\n- clarity\n",
        )
        .unwrap();
        let env = canon_add(&owox, "20260614", "brand", Some("values"), "honesty");
        assert_eq!(env.status, Status::Ok);
        assert!(!env.decision_ids.is_empty());
        let brand =
            Brand::from_markdown(&std::fs::read_to_string(owox.join("brand.md")).unwrap()).unwrap();
        assert!(brand.values.contains(&"honesty".to_string()));
        assert!(brand.values.contains(&"clarity".to_string()));
    }

    #[test]
    fn add_brand_creates_missing_section() {
        let owox = tempdir();
        std::fs::write(owox.join("brand.md"), "## Vision\n\nShip it.\n").unwrap();
        let env = canon_add(
            &owox,
            "20260614",
            "brand",
            Some("principles"),
            "small steps",
        );
        assert_eq!(env.status, Status::Ok);
        let brand =
            Brand::from_markdown(&std::fs::read_to_string(owox.join("brand.md")).unwrap()).unwrap();
        assert!(brand.principles.contains(&"small steps".to_string()));
    }

    #[test]
    fn add_rules_unknown_section_fails_and_discloses_valid() {
        let owox = tempdir();
        let env = canon_add(&owox, "20260614", "rules", Some("irreversible"), "x");
        assert_eq!(env.status, Status::Failed);
        // 未知 section 時は有効値を返し AI が正しく再試行できる。
        let valid = env.data.unwrap()["valid_sections"].clone();
        assert_eq!(
            valid,
            serde_json::json!([
                "change-policy",
                "dependency-policy",
                "deletion-policy",
                "safety"
            ])
        );
    }

    #[test]
    fn add_brand_missing_section_discloses_valid() {
        let owox = tempdir();
        let env = canon_add(&owox, "20260614", "brand", None, "x");
        assert_eq!(env.status, Status::Failed);
        assert!(env.data.unwrap()["valid_sections"].is_array());
    }

    #[test]
    fn add_glossary_via_text_pair() {
        let owox = tempdir();
        let env = canon_add(
            &owox,
            "20260614",
            "glossary",
            None,
            "canon: source of truth",
        );
        assert_eq!(env.status, Status::Ok);
    }

    #[test]
    fn add_practices_routes_to_practices() {
        let owox = tempdir();
        let env = canon_add(&owox, "20260614", "practices", None, "prefer small diffs");
        assert_eq!(env.status, Status::Ok);
        let p =
            Practices::from_markdown(&std::fs::read_to_string(owox.join("practices.md")).unwrap())
                .unwrap();
        assert_eq!(p.entries.len(), 1);
    }

    /// 自由文の提案 (op 無し)。
    fn free<'a>(target: &'a str, change: &'a str) -> ProposeInput<'a> {
        ProposeInput {
            target,
            op: None,
            section: None,
            item: None,
            to: None,
            change: Some(change),
        }
    }

    #[test]
    fn propose_free_text_is_human_gate() {
        let owox = tempdir();
        for target in ["brand", "rules", "practices", "glossary"] {
            let env = canon_propose(&owox, "20260614", free(target, "remove the stale item"));
            assert_eq!(env.status, Status::NeedsHuman, "{target}");
            assert!(env.gate.is_some());
            // 自由文には適用変更を紐づけない (人間が手編集して承認)。
            let id = &env.decision_ids[0];
            let d = crate::record::load_decision(&owox, id).unwrap();
            assert!(d.proposed_change.is_none(), "{target}");
        }
    }

    #[test]
    fn propose_unknown_target_fails() {
        let owox = tempdir();
        assert_eq!(
            canon_propose(&owox, "20260614", free("quality", "x")).status,
            Status::Failed
        );
    }

    #[test]
    fn propose_without_op_or_change_fails() {
        let owox = tempdir();
        let env = canon_propose(
            &owox,
            "20260614",
            ProposeInput {
                target: "rules",
                op: None,
                section: None,
                item: None,
                to: None,
                change: None,
            },
        );
        assert_eq!(env.status, Status::Failed);
    }

    #[test]
    fn propose_remove_missing_item_discloses_items() {
        let owox = tempdir();
        std::fs::write(
            owox.join("rules.md"),
            "## Deletion policy\n\n- keep history\n- drop temp files\n",
        )
        .unwrap();
        let env = canon_propose(
            &owox,
            "20260614",
            ProposeInput {
                target: "rules",
                op: Some("remove"),
                section: Some("deletion-policy"),
                item: Some("no such line"),
                to: None,
                change: None,
            },
        );
        // 一致なし → failed・現項目を data で開示・ゲートは作らない。
        assert_eq!(env.status, Status::Failed);
        let items = env.data.unwrap()["items"].clone();
        assert_eq!(
            items,
            serde_json::json!(["keep history", "drop temp files"])
        );
        assert!(env.decision_ids.is_empty());
    }

    #[test]
    fn propose_then_approve_applies_remove() {
        let owox = tempdir();
        std::fs::write(
            owox.join("rules.md"),
            "## Deletion policy\n\n- keep history\n- drop temp files\n",
        )
        .unwrap();
        let env = canon_propose(
            &owox,
            "20260614",
            ProposeInput {
                target: "rules",
                op: Some("remove"),
                section: Some("deletion-policy"),
                item: Some("drop temp files"),
                to: None,
                change: None,
            },
        );
        assert_eq!(env.status, Status::NeedsHuman);
        // この時点では canon 不変。
        let before = std::fs::read_to_string(owox.join("rules.md")).unwrap();
        assert!(before.contains("drop temp files"));

        // 承認相当: 適用を実行。
        let id = &env.decision_ids[0];
        apply_pending_canon_change(&owox, id).unwrap();
        let after = std::fs::read_to_string(owox.join("rules.md")).unwrap();
        assert!(!after.contains("drop temp files"));
        assert!(after.contains("keep history"));
    }

    #[test]
    fn propose_then_approve_applies_replace_glossary_by_term() {
        let owox = tempdir();
        std::fs::write(
            owox.join("glossary.md"),
            "## Terms\n\n- canon: old definition\n- gate: a human checkpoint\n",
        )
        .unwrap();
        // glossary は用語名で引ける。
        let env = canon_propose(
            &owox,
            "20260614",
            ProposeInput {
                target: "glossary",
                op: Some("replace"),
                section: None,
                item: Some("canon"),
                to: Some("canon: the source of truth"),
                change: None,
            },
        );
        assert_eq!(env.status, Status::NeedsHuman);
        let id = &env.decision_ids[0];
        apply_pending_canon_change(&owox, id).unwrap();
        let after = std::fs::read_to_string(owox.join("glossary.md")).unwrap();
        assert!(after.contains("- canon: the source of truth"));
        assert!(!after.contains("old definition"));
    }

    #[test]
    fn apply_no_change_is_noop() {
        // 通常のゲート (proposed_change 無し) では apply は何もしない。
        let owox = tempdir();
        let rec = record_decision(
            &owox,
            "20260614",
            RecordInput {
                title: "plain gate".to_string(),
                status: DecisionStatus::Open,
                rationale: "x".to_string(),
                links: DecisionLinks::default(),
                supersedes: Vec::new(),
            },
        );
        let id = &rec.decision_ids[0];
        assert!(apply_pending_canon_change(&owox, id).is_ok());
    }

    #[test]
    fn replace_requires_to() {
        let owox = tempdir();
        std::fs::write(owox.join("rules.md"), "## Safety\n\n- never push --force\n").unwrap();
        let env = canon_propose(
            &owox,
            "20260614",
            ProposeInput {
                target: "rules",
                op: Some("replace"),
                section: Some("safety"),
                item: Some("never push --force"),
                to: None,
                change: None,
            },
        );
        assert_eq!(env.status, Status::Failed);
    }

    #[test]
    fn correction_drafts_open_practice_gate_and_apply_adds_it() {
        let owox = tempdir();
        std::fs::write(owox.join("practices.md"), "## Practices\n\n- existing\n").unwrap();
        let env = propose_practice_from_correction(
            &owox,
            "20260619",
            "used English in a Japanese sentence",
            "keep prose in one language",
        );
        // 草案は人間ゲート。practices はまだ変わらない (固定は承認後)。
        assert_eq!(env.status, Status::NeedsHuman);
        assert!(
            !std::fs::read_to_string(owox.join("practices.md"))
                .unwrap()
                .contains("keep prose in one language")
        );
        // 承認相当 (op=add の適用) で practices へ載る。
        let id = &env.decision_ids[0];
        apply_pending_canon_change(&owox, id).unwrap();
        let after = std::fs::read_to_string(owox.join("practices.md")).unwrap();
        assert!(after.contains("keep prose in one language"));
        assert!(after.contains("existing"));
    }

    #[test]
    fn revert_add_removes_the_added_practice() {
        let owox = tempdir();
        std::fs::write(owox.join("practices.md"), "## Practices\n\n- existing\n").unwrap();
        let env = propose_practice_from_correction(&owox, "20260619", "", "small diffs");
        let id = &env.decision_ids[0];
        apply_pending_canon_change(&owox, id).unwrap();
        assert!(
            std::fs::read_to_string(owox.join("practices.md"))
                .unwrap()
                .contains("small diffs")
        );
        // 差し戻すと追加項目が消え、元の項目は残る。
        revert_pending_canon_change(&owox, id).unwrap();
        let after = std::fs::read_to_string(owox.join("practices.md")).unwrap();
        assert!(!after.contains("small diffs"));
        assert!(after.contains("existing"));
    }

    #[test]
    fn revert_replace_restores_original() {
        let owox = tempdir();
        std::fs::write(
            owox.join("rules.md"),
            "## Safety\n\n- never push --force\n",
        )
        .unwrap();
        let env = canon_propose(
            &owox,
            "20260619",
            ProposeInput {
                target: "rules",
                op: Some("replace"),
                section: Some("safety"),
                item: Some("never push --force"),
                to: Some("never force-push to shared branches"),
                change: None,
            },
        );
        let id = &env.decision_ids[0];
        apply_pending_canon_change(&owox, id).unwrap();
        assert!(
            std::fs::read_to_string(owox.join("rules.md"))
                .unwrap()
                .contains("never force-push to shared branches")
        );
        // 差し戻すと元の文面へ戻る。
        revert_pending_canon_change(&owox, id).unwrap();
        let after = std::fs::read_to_string(owox.join("rules.md")).unwrap();
        assert!(after.contains("never push --force"));
        assert!(!after.contains("never force-push to shared branches"));
    }
}
