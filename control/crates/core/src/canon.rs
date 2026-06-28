//! 統一 canon 編集モデル (`docs/decisions/20260614-Phase7-経験IOと二層ルール.md` の追補)。
//!
//! brand / rules / practices / glossary を同じ作法で編集する:
//! - add (追加): AI 直接 + 来歴。追加は既存の方向を壊さない低リスク
//! - change / remove (変更・削除): AI 提案 → open gate → needs_human。canon は直接書かない
//!
//! 内部は per-target の実装 (glossary::add・practices::add・brand/rules への追記) を再利用し、
//! 入口だけ統一する。固定 (brand/rules/glossary) と成長 (practices) は内容の差で、編集の作法は同一。
//!
//! structured backend (`docs/decisions/20260628-設定の統一canon化.md`):
//! TOML 設定 (config / quality / release / agents) は全操作が人間ゲート。
//! canon.add では受けず、canon.propose のみで構造化引数を受け gate → apply_structured_change。

use std::path::Path;

use crate::envelope::{Envelope, Gate};
use crate::record::{
    DecisionLinks, DecisionStatus, ProposedChange, RecordInput, load_decision, record_decision,
    record_decision_with_change,
};

/// structured backend の target かを判定する。
///
/// structured backend: config / quality / release / agents。
/// 全操作が人間ゲート (canon.propose のみ・canon.add では受けない)。
fn is_structured_target(target: &str) -> bool {
    matches!(target, "config" | "quality" | "release" | "agents")
}

/// target → ファイル名と再検証クロージャの対応。
/// config → config.toml、quality → quality.toml、release → release.toml、agents → agents.toml。
fn structured_file(target: &str) -> Option<&'static str> {
    match target {
        "config" => Some("config.toml"),
        "quality" => Some("quality.toml"),
        "release" => Some("release.toml"),
        "agents" => Some("agents.toml"),
        _ => None,
    }
}

/// agents.toml の役割 id として有効な 5 つ。
const BUILTIN_ROLE_IDS: &[&str] = &["investigate", "plan", "implement", "review", "verify"];

/// canon.add。canon へ項目を 1 件 AI 直接追加し、来歴へ記録する。
///
/// target = brand / rules / practices / glossary。
/// brand / rules は section (どの一覧か) が要る。glossary は text = "用語: 定義"。practices は text = 指針。
/// structured target (config 等) は canon.add を受けない → canon.propose へ案内する。
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
    // structured backend は全操作が人間ゲート。add はここで弾く。
    if is_structured_target(target.trim()) {
        return Envelope::failed(format!(
            "canon.add は {target} を受けない。structured 設定の変更は全て人間ゲート: canon.propose を使う (op=add/remove/replace・heading=設定パス・item=識別子・to=新値)"
        ));
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
        "context" => add_to_context(owox_dir, today, section, text),
        other => Envelope::failed(format!(
            "canon.add の target は brand / rules / practices / glossary / context: {other}"
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
///
/// structured backend (config 等) は op が必須。自由文提案は受けない。
pub fn canon_propose(owox_dir: &Path, today: &str, input: ProposeInput) -> Envelope {
    let target = input.target.trim();

    // structured backend: prose とは別の照合・書き戻し経路へ分岐する。
    if is_structured_target(target) {
        return propose_structured_config(owox_dir, today, target, &input);
    }

    let Some(file) = target_file(target) else {
        return Envelope::failed(format!(
            "canon.propose の target は brand / rules / practices / glossary / context / config: {target}"
        ));
    };

    // context backend: セクションブロック (## スコープ) 単位の remove / replace。
    // 箇条書き項目の照合ではなくスコープ見出しで照合するため専用経路へ分岐する。
    if target == "context" {
        if let Some(op) = input.op.map(str::trim).filter(|s| !s.is_empty()) {
            return propose_context(owox_dir, today, file, op, &input);
        }
        let change = input.change.unwrap_or("").trim();
        if change.is_empty() {
            return Envelope::failed(
                "op (remove / replace) で具体的な変更を出すか、change で自由文の提案を出す",
            );
        }
        // 自由文提案は prose と同じ経路。
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
        return Envelope::needs_human(
            format!(
                "Changing or removing part of {target} is a decision for a human. Recorded an open gate; this free-text proposal needs a human to edit {file} and approve. For a removal or replacement of one entry, pass op=remove/replace with item=scope heading."
            ),
            Gate {
                kind: "canon".to_string(),
                subject: format!("{target} change"),
                requires: format!(
                    "Edit {file} in the canon for this free-text proposal, then call gate.approve yourself; its CLI confirmation prompt is the human's approval. Or the human rejects it."
                ),
            },
        )
        .with_decision_ids(rec.decision_ids);
    }

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
/// structured target (config 等) は apply_structured_change へ分岐する。
pub fn apply_pending_canon_change(owox_dir: &Path, id: &str) -> Result<(), String> {
    let decision = load_decision(owox_dir, id)?;
    if decision.status != DecisionStatus::Open {
        return Ok(());
    }
    let Some(change) = decision.proposed_change else {
        return Ok(());
    };

    // structured backend: TOML 書き戻し経路へ分岐する。
    if is_structured_target(&change.target) {
        return apply_structured_change(owox_dir, &change);
    }

    // context backend: セクションブロック単位の編集経路へ分岐する。
    if change.target == "context" {
        return apply_context_change(owox_dir, &change);
    }

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
/// structured target は v1 では逆適用未対応 → Err を返し差し戻させない (データ破壊を防ぐ)。
pub fn revert_pending_canon_change(owox_dir: &Path, id: &str) -> Result<(), String> {
    let decision = load_decision(owox_dir, id)?;
    let Some(change) = decision.proposed_change else {
        return Ok(());
    };

    // structured backend は v1 で revert 未対応。誤って戻し書きしないよう明示 Err。
    if is_structured_target(&change.target) {
        return Err(format!(
            "structured revert not supported in v1: {} の変更を手動で戻してください",
            change.target
        ));
    }

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

/// remove / replace 時の事前検証: 配列テーブルに item が存在するかを確認する。
///
/// 存在しなければ Some(Envelope::failed) を返す。存在すれば None。
/// ゲートを開く前に fail-fast することで無効なゲートを残さない。
fn preflight_array_item(
    owox_dir: &Path,
    file: &str,
    target: &str,
    heading: &str,
    item: &str,
) -> Option<Envelope> {
    let path = owox_dir.join(file);
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    if text.trim().is_empty() {
        // ファイルが空なら配列は存在しない → エントリも無い。
        return Some(Envelope::failed(format!(
            "{file} が空または存在しない: {heading} に {item} は見つからない"
        )));
    }
    // target のパーサで読み、id_field 照合で存在確認する。
    // config: verify.checks → VerifyConfig; quality: layers → Quality; release: checks/artifacts → Release。
    let names: Vec<String> = match (target, heading) {
        ("config", "verify.checks") => match crate::model::VerifyConfig::from_toml(&text) {
            Ok(vc) => vc.checks.into_iter().map(|c| c.name).collect(),
            Err(e) => return Some(Envelope::failed(format!("{file} を解釈できない: {e}"))),
        },
        ("quality", "layers") => match crate::quality::Quality::from_toml(&text) {
            Ok(q) => q.layers.into_iter().filter_map(|l| l.name).collect(),
            Err(e) => return Some(Envelope::failed(format!("{file} を解釈できない: {e}"))),
        },
        ("release", "checks") => match crate::release::Release::from_toml(&text) {
            Ok(r) => r.checks.into_iter().map(|c| c.name).collect(),
            Err(e) => return Some(Envelope::failed(format!("{file} を解釈できない: {e}"))),
        },
        ("release", "artifacts") => match crate::release::Release::from_toml(&text) {
            Ok(r) => r.artifacts,
            Err(e) => return Some(Envelope::failed(format!("{file} を解釈できない: {e}"))),
        },
        _ => return None, // スカラ値など照合不要
    };
    if !names.iter().any(|n| n == item) {
        return Some(
            Envelope::failed(format!("{heading} に {item} が見つからない")).with_data(
                serde_json::json!({
                    "target": target,
                    "heading": heading,
                    "available_names": names,
                }),
            ),
        );
    }
    None
}

/// structured backend: config / quality / release / agents の変更提案を人間ゲートとして記録する。
///
/// op = add / remove / replace が必須。自由文提案 (change のみ) は受けない。
/// ProposedChange のフィールドを structured 意味に転用する:
/// - target  = "config" | "quality" | "release" | "agents"
/// - heading = 設定パス (例: "settings.language" / "verify.checks" / "layers" / "policy" / "roles" / "variants")
/// - op      = "add" | "remove" | "replace"
/// - item    = 識別子値 (配列テーブルは name / 単一値は key 名 / roles は role id)
/// - to      = 新値 (add/replace の時だけ必須)。配列テーブルは 1 行のインラインテーブル。単一値は素のスカラ。
///
/// 対応 heading 一覧:
///   config:  settings.language / verify.checks
///   quality: layers (key=name) / bulk_delete_threshold / delivery.always_limit /
///            decay.<scalar整数フィールド>
///   release: checks (key=name) / artifacts (key=path) / policy / version.file / version.pattern
///   agents:  roles (キー付きマップ・op=replace/remove のみ) /
///            variants (配列テーブル id フィールド・op=add/remove/replace)
///
/// 非対応 (v1 defer):
///   quality: boundaries / budgets の remove / replace — paths が配列キーで単一識別子が無い。
///            add も非自明なため defer。deferred な操作は apply で Err を返し書き込まない。
fn propose_structured_config(
    owox_dir: &Path,
    today: &str,
    target: &str,
    input: &ProposeInput,
) -> Envelope {
    let file = match structured_file(target) {
        Some(f) => f,
        None => return Envelope::failed(format!("未知の structured target: {target}")),
    };

    let op = match input.op.map(str::trim).filter(|s| !s.is_empty()) {
        Some(op) => op,
        None => {
            return Envelope::failed("structured 設定の変更は op が必須 (add / remove / replace)");
        }
    };
    if op != "add" && op != "remove" && op != "replace" {
        return Envelope::failed(format!("op は add / remove / replace のいずれか: {op}"));
    }
    let heading = match input.section.map(str::trim).filter(|s| !s.is_empty()) {
        Some(h) => h,
        None => {
            return Envelope::failed(
                "structured 設定の変更は section (設定パス: 例 settings.language / verify.checks / layers) が要る",
            );
        }
    };
    let item = match input.item.map(str::trim).filter(|s| !s.is_empty()) {
        Some(i) => i,
        None => {
            return Envelope::failed(
                "structured 設定の変更は item (識別子: 配列テーブルなら name / 単一値なら key 名) が要る",
            );
        }
    };

    // agents は役割とバリアント別の専用検証経路へ分岐する。
    if target == "agents" {
        return propose_structured_agents(owox_dir, today, file, op, heading, item, input);
    }

    // heading ごとのインラインテーブル必須キーを返す。None = スカラ値。
    // ここで None の場合でも apply_single_value が型変換を試みる (整数・真偽・文字列の順)。
    let inline_required_keys: Option<&[&str]> = match (target, heading) {
        ("config", "verify.checks") => Some(&["name", "command"]),
        ("quality", "layers") => Some(&["name", "paths", "autonomy"]),
        ("release", "checks") => Some(&["name", "command"]),
        // release.toml の [[artifacts]] は ArtifactRaw { name } なので識別子は name。
        ("release", "artifacts") => Some(&["name"]),
        // quality boundaries / budgets: v1 defer (paths が配列キーで単一識別子が無い)。
        // add も非自明なため defer。apply_structured_change が明示 Err を返す。
        ("quality", "boundaries") | ("quality", "budgets") => {
            return Envelope::failed(
                "quality boundaries / budgets の編集は v1 では未対応 (defer): \
                paths フィールドが配列キーで単一文字列識別子が無いため専用照合設計が必要。\
                手動で quality.toml を編集してください",
            );
        }
        _ => None, // スカラ値 (settings.language / bulk_delete_threshold 等)
    };

    let to = if op == "add" || op == "replace" {
        match input.to.map(str::trim).filter(|s| !s.is_empty()) {
            Some(t) => {
                // 配列テーブルエントリは 1 行インラインテーブルで受ける。
                // スカラ値 (language / bulk_delete_threshold 等) は素のスカラ文字列のまま。
                if let Some(required) = inline_required_keys {
                    let entry = match parse_inline_entry(t, required) {
                        Ok(entry) => entry,
                        Err(err) => return Envelope::failed(err),
                    };
                    // item を唯一の識別子源に保つ: to の識別子フィールドは item と一致させる。
                    // apply 側の重複検査・照合は item で行うため、ここで一致を保証しないと
                    // 挿入されるエントリ識別子が item とずれ、検査が無意味になる。
                    let id_field = array_table_id_field(target, heading);
                    let to_id = entry
                        .get(id_field)
                        .and_then(|v| v.as_str())
                        .unwrap_or_default();
                    if to_id != item {
                        return Envelope::failed(format!(
                            "{heading} の item と to の {id_field} は一致させる (rename は remove + add)",
                        ));
                    }
                }
                Some(t.to_string())
            }
            None => {
                return Envelope::failed(format!("op={op} には to (新値) が要る"));
            }
        }
    } else {
        None
    };

    // remove / replace 時: 配列テーブルの場合は既存ファイルを読んで fail-fast する。
    // 対象エントリが存在しないままゲートを開いても apply が失敗するだけで無駄。
    if (op == "remove" || op == "replace")
        && inline_required_keys.is_some()
        && let Some(e) = preflight_array_item(owox_dir, file, target, heading, item)
    {
        return e;
    }

    let action = match &to {
        Some(to) => format!("{op} {heading}: item=\"{item}\" to={to}"),
        None => format!("{op} {heading}: item=\"{item}\""),
    };
    let rec = record_decision_with_change(
        owox_dir,
        today,
        RecordInput {
            title: format!("Propose {target} change: {heading}"),
            status: DecisionStatus::Open,
            rationale: format!("Proposed structured change to {file}. {action}"),
            links: DecisionLinks::default(),
            supersedes: Vec::new(),
        },
        Some(ProposedChange {
            target: target.to_string(),
            heading: heading.to_string(),
            op: op.to_string(),
            item: item.to_string(),
            to,
        }),
    );

    Envelope::needs_human(
        format!(
            "Changing {file} is a decision for a human. Recorded an open gate with the exact change ({action}). When the human decides to approve, call gate.approve yourself; its CLI confirmation prompt is the human's approval, and owox then applies the change to {file}. Do not edit {file} yourself."
        ),
        Gate {
            kind: "canon".to_string(),
            subject: format!("{target} {heading} {op}"),
            requires: format!(
                "When the human decides, call gate.approve; its CLI confirmation prompt is the human's approval and owox applies the change to {file}."
            ),
        },
    )
    .with_decision_ids(rec.decision_ids)
}

/// 配列テーブル heading の識別子フィールド名を返す。
/// config verify.checks / quality layers / release checks / release artifacts → "name"。
/// (release.toml の [[artifacts]] は ArtifactRaw { name } なので識別子は name)
fn array_table_id_field(_target: &str, _heading: &str) -> &'static str {
    "name"
}

/// agents.toml 専用の変更提案ハンドラ。
///
/// roles    : op = replace / remove のみ (add は5固定・fail-fast)。
///            item は組込み5役割 id の一つ (未知 id は fail-fast)。
///            replace の to はインラインテーブル。
/// variants : op = add / remove / replace。
///            item = 変種 id。
///            add / replace の to はインラインテーブル (id, applies_to, prompt が必須)。
///            to の id は item と一致させる (rename 禁止)。
///            remove / replace の fail-fast: ファイル内の variants から id を検索し無ければ Err。
fn propose_structured_agents(
    owox_dir: &Path,
    today: &str,
    file: &str,
    op: &str,
    heading: &str,
    item: &str,
    input: &ProposeInput,
) -> Envelope {
    match heading {
        "roles" => {
            // add は役割5固定のため不可。
            if op == "add" {
                return Envelope::failed(
                    "roles への add は不可: 役割は5固定 (investigate / plan / implement / review / verify)。\
                     既存役割のフィールドを上書きするには op=replace を使う",
                );
            }
            if op != "replace" && op != "remove" {
                return Envelope::failed(format!(
                    "roles の op は replace / remove のいずれか: {op}"
                ));
            }
            // item が組込み5役割の一つかを fail-fast で確認する。
            if !BUILTIN_ROLE_IDS.contains(&item) {
                return Envelope::failed(format!(
                    "未知の役割 id: {item} (組込み5役割のみ: {})",
                    BUILTIN_ROLE_IDS.join(" / ")
                ));
            }
            let to = if op == "replace" {
                match input.to.map(str::trim).filter(|s| !s.is_empty()) {
                    Some(t) => {
                        // to をインラインテーブルとして事前検証する (parse するだけで必須キーは無し)。
                        if let Err(e) = parse_inline_entry(t, &[]) {
                            return Envelope::failed(format!(
                                "to はインラインテーブルで渡す (例: {{ tier = \"strong\", sandbox = \"workspace-write\" }}): {e}"
                            ));
                        }
                        Some(t.to_string())
                    }
                    None => {
                        return Envelope::failed("op=replace には to (インラインテーブル) が要る");
                    }
                }
            } else {
                None // remove
            };
            let action = match &to {
                Some(to) => format!("{op} roles.{item}: to={to}"),
                None => format!("{op} roles.{item}"),
            };
            let rec = record_decision_with_change(
                owox_dir,
                today,
                RecordInput {
                    title: format!("Propose agents change: roles.{item}"),
                    status: DecisionStatus::Open,
                    rationale: format!("Proposed structured change to {file}. {action}"),
                    links: DecisionLinks::default(),
                    supersedes: Vec::new(),
                },
                Some(ProposedChange {
                    target: "agents".to_string(),
                    heading: "roles".to_string(),
                    op: op.to_string(),
                    item: item.to_string(),
                    to,
                }),
            );
            Envelope::needs_human(
                format!(
                    "Changing {file} is a decision for a human. Recorded an open gate with the exact change ({action}). When the human decides to approve, call gate.approve yourself; its CLI confirmation prompt is the human's approval, and owox then applies the change to {file}. Do not edit {file} yourself."
                ),
                Gate {
                    kind: "canon".to_string(),
                    subject: format!("agents roles.{item} {op}"),
                    requires: format!(
                        "When the human decides, call gate.approve; its CLI confirmation prompt is the human's approval and owox applies the change to {file}."
                    ),
                },
            )
            .with_decision_ids(rec.decision_ids)
        }
        "variants" => {
            if op != "add" && op != "remove" && op != "replace" {
                return Envelope::failed(format!(
                    "variants の op は add / remove / replace のいずれか: {op}"
                ));
            }
            // add / replace: to のインラインテーブル検証 + id 一致検査。
            let to = if op == "add" || op == "replace" {
                match input.to.map(str::trim).filter(|s| !s.is_empty()) {
                    Some(t) => {
                        let entry = match parse_inline_entry(t, &["id", "applies_to", "prompt"]) {
                            Ok(e) => e,
                            Err(e) => return Envelope::failed(e),
                        };
                        // to の id は item と一致させる (rename は remove + add)。
                        let to_id = entry.get("id").and_then(|v| v.as_str()).unwrap_or_default();
                        if to_id != item {
                            return Envelope::failed(format!(
                                "variants の item と to の id は一致させる (rename は remove + add): item={item}, to.id={to_id}"
                            ));
                        }
                        Some(t.to_string())
                    }
                    None => {
                        return Envelope::failed(format!(
                            "op={op} には to (インラインテーブル) が要る"
                        ));
                    }
                }
            } else {
                None // remove
            };

            // remove / replace の fail-fast: agents.toml 内の variants を検索して id がなければ Err。
            if op == "remove" || op == "replace" {
                let path = owox_dir.join(file);
                let text = std::fs::read_to_string(&path).unwrap_or_default();
                if !text.trim().is_empty() {
                    match crate::agents::Agents::from_toml(&text) {
                        Ok(agents) => {
                            // variants にプロジェクト定義の id があるか確認する。
                            // 組込み変種も対象に含む (from_toml でマージ済み)。
                            if !agents.variants.iter().any(|v| v.id == item) {
                                let available: Vec<String> =
                                    agents.variants.iter().map(|v| v.id.clone()).collect();
                                return Envelope::failed(format!(
                                    "variants に id=\"{item}\" が見つからない"
                                ))
                                .with_data(serde_json::json!({
                                    "target": "agents",
                                    "heading": "variants",
                                    "available_ids": available,
                                }));
                            }
                        }
                        Err(e) => {
                            return Envelope::failed(format!("{file} を解釈できない: {e}"));
                        }
                    }
                } else {
                    // ファイルが空 → プロジェクト定義変種は無し。組込みだけ対象にする。
                    // 組込み変種は adversarial / gardener。
                    let builtins = ["adversarial", "gardener"];
                    if !builtins.contains(&item) {
                        return Envelope::failed(format!(
                            "variants に id=\"{item}\" が見つからない (agents.toml が空: 組込み variants は {})",
                            builtins.join(" / ")
                        ))
                        .with_data(serde_json::json!({
                            "target": "agents",
                            "heading": "variants",
                            "available_ids": builtins,
                        }));
                    }
                }
            }

            let action = match &to {
                Some(to) => format!("{op} variants id=\"{item}\": to={to}"),
                None => format!("{op} variants id=\"{item}\""),
            };
            let rec = record_decision_with_change(
                owox_dir,
                today,
                RecordInput {
                    title: format!("Propose agents change: variants.{item}"),
                    status: DecisionStatus::Open,
                    rationale: format!("Proposed structured change to {file}. {action}"),
                    links: DecisionLinks::default(),
                    supersedes: Vec::new(),
                },
                Some(ProposedChange {
                    target: "agents".to_string(),
                    heading: "variants".to_string(),
                    op: op.to_string(),
                    item: item.to_string(),
                    to,
                }),
            );
            Envelope::needs_human(
                format!(
                    "Changing {file} is a decision for a human. Recorded an open gate with the exact change ({action}). When the human decides to approve, call gate.approve yourself; its CLI confirmation prompt is the human's approval, and owox then applies the change to {file}. Do not edit {file} yourself."
                ),
                Gate {
                    kind: "canon".to_string(),
                    subject: format!("agents variants.{item} {op}"),
                    requires: format!(
                        "When the human decides, call gate.approve; its CLI confirmation prompt is the human's approval and owox applies the change to {file}."
                    ),
                },
            )
            .with_decision_ids(rec.decision_ids)
        }
        other => Envelope::failed(format!(
            "未対応の agents heading: {other} (対応: roles / variants)"
        )),
    }
}

/// structured backend: ProposedChange を target に対応するファイルへ適用する。
///
/// - target → ファイル名 (structured_file) を解決
/// - ファイルを読み toml::Value へパース
/// - op と heading で対象を特定し値を操作
/// - target に対応するパーサで再検証 (スキーマ安全網)
/// - 検証が通ったら書き戻す
fn apply_structured_change(owox_dir: &Path, change: &ProposedChange) -> Result<(), String> {
    let file = structured_file(&change.target)
        .ok_or_else(|| format!("未知の structured target: {}", change.target))?;
    let path = owox_dir.join(file);
    let text = std::fs::read_to_string(&path)
        .map_err(|e| format!("{} を読めない: {e}", path.display()))?;

    let mut doc: toml::Value = if text.trim().is_empty() {
        toml::Value::Table(toml::map::Map::new())
    } else {
        toml::from_str(&text).map_err(|e| format!("{file} を解釈できない: {e}"))?
    };

    // ProposedChange.to は 1 行 (スカラ or インラインテーブル) で保存済み。エスケープ無し。
    match change.target.as_str() {
        "config" => apply_config_change(&mut doc, change)?,
        "quality" => apply_quality_change(&mut doc, change)?,
        "release" => apply_release_change(&mut doc, change)?,
        "agents" => apply_agents_change(&mut doc, change)?,
        other => return Err(format!("未知の structured target: {other}")),
    }

    // スキーマ再検証: 書き戻す前に既存パーサで通ることを確認する。
    let updated_text =
        toml::to_string(&doc).map_err(|e| format!("{file} を文字列化できない: {e}"))?;
    match change.target.as_str() {
        "config" => {
            crate::model::Settings::from_toml(&updated_text)
                .map_err(|e| format!("変更後の {file} がスキーマ検証で失敗: {e}"))?;
            crate::model::VerifyConfig::from_toml(&updated_text)
                .map_err(|e| format!("変更後の {file} がスキーマ検証で失敗: {e}"))?;
        }
        "quality" => {
            crate::quality::Quality::from_toml(&updated_text)
                .map_err(|e| format!("変更後の {file} がスキーマ検証で失敗: {e}"))?;
        }
        "release" => {
            crate::release::Release::from_toml(&updated_text)
                .map_err(|e| format!("変更後の {file} がスキーマ検証で失敗: {e}"))?;
        }
        "agents" => {
            crate::agents::Agents::from_toml(&updated_text)
                .map_err(|e| format!("変更後の {file} がスキーマ検証で失敗: {e}"))?;
        }
        _ => {}
    }

    std::fs::write(&path, updated_text)
        .map_err(|e| format!("{} へ書けない: {e}", path.display()))?;
    Ok(())
}

/// config.toml の heading ディスパッチ。
fn apply_config_change(doc: &mut toml::Value, change: &ProposedChange) -> Result<(), String> {
    match change.heading.as_str() {
        // 単一値: settings.language は ConfigRaw のトップレベル `language` キー。
        "settings.language" => {
            apply_single_value(doc, &["language"], &change.op, &change.to)?;
        }
        // 配列テーブル: verify.checks (name で照合)
        "verify.checks" => {
            apply_array_table(
                doc,
                &["verify", "checks"],
                "name",
                &["name", "command"],
                &change.op,
                &change.item,
                change.to.as_deref(),
            )?;
        }
        other => {
            return Err(format!(
                "未対応の config heading: {other} (対応: settings.language / verify.checks)"
            ));
        }
    }
    Ok(())
}

/// quality.toml の heading ディスパッチ。
///
/// 対応: layers (key=name) / bulk_delete_threshold / delivery.always_limit /
///        decay.<scalar整数フィールド>
/// 非対応 (v1 defer): boundaries / budgets — paths が配列キーで単一識別子が無い。
fn apply_quality_change(doc: &mut toml::Value, change: &ProposedChange) -> Result<(), String> {
    match change.heading.as_str() {
        // 配列テーブル: layers (name で照合)
        "layers" => {
            apply_array_table(
                doc,
                &["layers"],
                "name",
                &["name", "paths", "autonomy"],
                &change.op,
                &change.item,
                change.to.as_deref(),
            )?;
        }
        // トップレベルスカラ整数
        "bulk_delete_threshold" => {
            apply_single_value(doc, &["bulk_delete_threshold"], &change.op, &change.to)?;
        }
        // 2 階層スカラ: [delivery] 配下
        "delivery.always_limit" => {
            apply_single_value(doc, &["delivery", "always_limit"], &change.op, &change.to)?;
        }
        // 2 階層スカラ: [decay] 配下の整数フィールド
        "decay.stale_task_days"
        | "decay.open_decision_days"
        | "decay.review_decision_days"
        | "decay.knowledge_stale_days"
        | "decay.branch_memory_stale_days"
        | "decay.min_duplicate_bytes"
        | "decay.gardening_floor_bloat_tokens"
        | "decay.gardening_low_use_days"
        | "decay.gardening_low_use_commits" => {
            let key = change.heading.strip_prefix("decay.").unwrap();
            apply_single_value(doc, &["decay", key], &change.op, &change.to)?;
        }
        // v1 defer: boundaries / budgets は paths が配列キーで単一識別子が無い専用設計が必要。
        "boundaries" | "budgets" => {
            return Err(format!(
                "quality {} の編集は v1 では未対応 (defer): \
                paths フィールドが配列キーで単一文字列識別子が無いため専用照合設計が必要。\
                手動で quality.toml を編集してください",
                change.heading
            ));
        }
        other => {
            return Err(format!(
                "未対応の quality heading: {other} \
                (対応: layers / bulk_delete_threshold / delivery.always_limit / decay.<フィールド>)"
            ));
        }
    }
    Ok(())
}

/// release.toml の heading ディスパッチ。
///
/// 対応: checks (key=name) / artifacts (key=path) / policy / version.file / version.pattern。
fn apply_release_change(doc: &mut toml::Value, change: &ProposedChange) -> Result<(), String> {
    match change.heading.as_str() {
        // 配列テーブル: checks (name で照合)
        "checks" => {
            apply_array_table(
                doc,
                &["checks"],
                "name",
                &["name", "command"],
                &change.op,
                &change.item,
                change.to.as_deref(),
            )?;
        }
        // 配列テーブル: artifacts (name で照合)
        // release.toml の [[artifacts]] は ArtifactRaw { name } で、識別子フィールドは name。
        // Release::from_toml は artifacts を Vec<String> (name を収集) へ変換する。
        "artifacts" => {
            apply_array_table(
                doc,
                &["artifacts"],
                "name",
                &["name"],
                &change.op,
                &change.item,
                change.to.as_deref(),
            )?;
        }
        // スカラ配列: policy (文字列の配列)
        "policy" => {
            apply_scalar_array(
                doc,
                &["policy"],
                &change.op,
                &change.item,
                change.to.as_deref(),
            )?;
        }
        // 2 階層スカラ: [version] 配下
        "version.file" => {
            apply_single_value(doc, &["version", "file"], &change.op, &change.to)?;
        }
        "version.pattern" => {
            apply_single_value(doc, &["version", "pattern"], &change.op, &change.to)?;
        }
        other => {
            return Err(format!(
                "未対応の release heading: {other} \
                (対応: checks / artifacts / policy / version.file / version.pattern)"
            ));
        }
    }
    Ok(())
}

/// agents.toml の heading ディスパッチ。
///
/// 対応 heading:
///   roles    → キー付きマップ `[roles.<id>]`。op = replace / remove のみ (add は不可・5固定)。
///   variants → 配列テーブル `[[variants]]` (id で照合)。op = add / remove / replace。
fn apply_agents_change(doc: &mut toml::Value, change: &ProposedChange) -> Result<(), String> {
    match change.heading.as_str() {
        "roles" => {
            // roles は replace (部分上書き) / remove (既定へ戻す) のみ。add は固定5なので不可。
            apply_keyed_table(doc, "roles", &change.op, &change.item, change.to.as_deref())?;
        }
        "variants" => {
            // variants は配列テーブル (id フィールドで照合)。add / remove / replace 全対応。
            // 必須フィールド: id, applies_to, prompt (tier_override は任意)。
            apply_array_table(
                doc,
                &["variants"],
                "id",
                &["id", "applies_to", "prompt"],
                &change.op,
                &change.item,
                change.to.as_deref(),
            )?;
        }
        other => {
            return Err(format!(
                "未対応の agents heading: {other} (対応: roles / variants)"
            ));
        }
    }
    Ok(())
}

/// TOML キー付きマップ (`[table_key.<id>]` 形式) を操作する汎用ヘルパ。
///
/// agents.toml の `[roles.<id>]` サブテーブルを想定している。
/// - op "replace": `[table_key.<id>]` サブテーブルを to インラインテーブルのフィールドで上書きする。
///   存在しないサブテーブルも新設して書く (部分上書きなので既存フィールドは残す)。
/// - op "remove": `[table_key.<id>]` サブテーブルを丸ごと削除する (役割を組込み既定へ戻す)。
/// - op "add": 役割は5固定なので弾く。
///
/// 注: Agents::from_toml 再検証が未知 id や不正フィールド値を弾く安全網として機能する。
fn apply_keyed_table(
    doc: &mut toml::Value,
    table_key: &str,
    op: &str,
    id: &str,
    to: Option<&str>,
) -> Result<(), String> {
    match op {
        "add" => {
            return Err("roles への add は不可: 役割は5固定 (investigate / plan / implement / review / verify)。\
                 既存役割のフィールドを上書きするには op=replace を使う"
                .to_string());
        }
        "replace" => {
            let to_str = to.ok_or("op=replace には to (インラインテーブル) が要る")?;
            // to をインラインテーブルとして解釈する (必須キーは無し・Agents::from_toml が検証)。
            let entry = parse_inline_entry(to_str, &[])?;
            let entry_table = match entry {
                toml::Value::Table(t) => t,
                _ => return Err("to はインラインテーブルで渡す".to_string()),
            };
            // doc のトップレベル [table_key] テーブルを取得または新設する。
            let outer = match doc {
                toml::Value::Table(t) => t,
                _ => return Err("TOML のトップレベルがテーブルでない".to_string()),
            };
            let roles_table = outer
                .entry(table_key.to_string())
                .or_insert_with(|| toml::Value::Table(toml::map::Map::new()));
            let roles_map = match roles_table {
                toml::Value::Table(t) => t,
                _ => return Err(format!("[{table_key}] がテーブルでない")),
            };
            // [table_key.<id>] サブテーブルを取得または新設して、to のフィールドを上書きする。
            let role_entry = roles_map
                .entry(id.to_string())
                .or_insert_with(|| toml::Value::Table(toml::map::Map::new()));
            let role_map = match role_entry {
                toml::Value::Table(t) => t,
                _ => return Err(format!("[{table_key}.{id}] がテーブルでない")),
            };
            for (k, v) in entry_table {
                role_map.insert(k, v);
            }
        }
        "remove" => {
            let outer = match doc {
                toml::Value::Table(t) => t,
                _ => return Err("TOML のトップレベルがテーブルでない".to_string()),
            };
            if let Some(toml::Value::Table(roles_map)) = outer.get_mut(table_key) {
                // サブテーブルが存在しなくてもエラーにしない (既に既定値のため)。
                roles_map.remove(id);
                // [roles] テーブルが空になったら丸ごと削除する。
                if roles_map.is_empty() {
                    outer.remove(table_key);
                }
            }
            // [roles] 自体が無い場合も正常 (既に既定値なので何もしない)。
        }
        other => return Err(format!("未知の op: {other}")),
    }
    Ok(())
}

/// TOML の単一値を add / remove / replace する。
/// path は 1 階層 (["language"]) または 2 階層 (["delivery", "always_limit"]) のキー配列。
///
/// to の文字列を整数 → 真偽値 → 文字列の順で解釈し適切な toml::Value を挿入する。
/// スキーマ再検証 (apply_structured_change) が最終的な型チェックを行う。
fn apply_single_value(
    doc: &mut toml::Value,
    path: &[&str],
    op: &str,
    to: &Option<String>,
) -> Result<(), String> {
    /// to 文字列を toml::Value へ変換する (整数 → 真偽値 → 文字列の順)。
    fn parse_scalar(val: &str) -> toml::Value {
        if let Ok(i) = val.parse::<i64>() {
            return toml::Value::Integer(i);
        }
        if let Ok(b) = val.parse::<bool>() {
            return toml::Value::Boolean(b);
        }
        toml::Value::String(val.to_string())
    }

    let table = match doc {
        toml::Value::Table(t) => t,
        _ => return Err("TOML のトップレベルがテーブルでない".to_string()),
    };
    if path.len() == 1 {
        // トップレベルキー直接操作 (例: language / bulk_delete_threshold)。
        let key = path[0];
        match op {
            "add" | "replace" => {
                let val = to.as_deref().ok_or("to が無い")?;
                table.insert(key.to_string(), parse_scalar(val));
            }
            "remove" => {
                table.remove(key);
            }
            other => return Err(format!("未知の op: {other}")),
        }
        Ok(())
    } else if path.len() == 2 {
        // ネストしたテーブルキー (例: [delivery] の always_limit / [version] の file)。
        let (outer_key, inner_key) = (path[0], path[1]);
        let outer = table
            .entry(outer_key.to_string())
            .or_insert_with(|| toml::Value::Table(toml::map::Map::new()));
        let inner_table = match outer {
            toml::Value::Table(t) => t,
            _ => {
                return Err(format!("[{outer_key}] がテーブルでない"));
            }
        };
        match op {
            "add" | "replace" => {
                let val = to.as_deref().ok_or("to が無い")?;
                inner_table.insert(inner_key.to_string(), parse_scalar(val));
            }
            "remove" => {
                inner_table.remove(inner_key);
            }
            other => return Err(format!("未知の op: {other}")),
        }
        Ok(())
    } else {
        Err(format!(
            "apply_single_value: 対応していないパス長 {}",
            path.len()
        ))
    }
}

/// structured エントリの 1 行インラインテーブルを解釈し、必須キーを確認する。
///
/// to = `{ name = "build", command = "cargo build" }` のような 1 行 TOML を、
/// `x = <to>` で包んで解釈し、x の値 (テーブル) を返す。
/// propose 時の事前検証と apply 時の構築で共有する (照合する識別子の取り違えを防ぐ)。
fn parse_inline_entry(to: &str, required: &[&str]) -> Result<toml::Value, String> {
    let wrapped = format!("x = {to}\n");
    let parsed: toml::Value = toml::from_str(&wrapped).map_err(|e| {
        format!("to はインラインテーブルで渡す (例: {{ name = \"build\", command = \"cargo build\" }}): {e}")
    })?;
    let entry = parsed.get("x").cloned().ok_or("to の解釈に失敗")?;
    if !entry.is_table() {
        return Err("to はインラインテーブル ({ key = value, ... }) で渡す".to_string());
    }
    for key in required {
        if entry.get(key).is_none() {
            return Err(format!("to のインラインテーブルに {key} フィールドが要る"));
        }
    }
    Ok(entry)
}

/// TOML 配列テーブルを操作する汎用ヘルパ。
///
/// - path: ドキュメントルートからのキー列 (例: ["verify", "checks"] / ["layers"])。
/// - id_field: 照合に使うフィールド名 (例: "name" / "path")。
/// - required: add/replace 時にインラインテーブルが持つべき必須フィールド名列。
/// - op: "add" | "remove" | "replace"。
/// - item: 識別子値 (id_field の値)。
/// - to: add/replace 時のインラインテーブル文字列。
///
/// add: id_field の重複検査後に末尾追加。
/// remove: id_field で最初に一致するエントリを削除。
/// replace: id_field で最初に一致するエントリを to で置き換え。
fn apply_array_table(
    doc: &mut toml::Value,
    path: &[&str],
    id_field: &str,
    required: &[&str],
    op: &str,
    item: &str,
    to: Option<&str>,
) -> Result<(), String> {
    // path を辿りながら中間テーブルを取得または新設する。
    let arr = {
        let mut cur = doc;
        let (parents, last) = path.split_at(path.len() - 1);
        for key in parents {
            let t = match cur {
                toml::Value::Table(t) => t,
                _ => return Err(format!("[{key}] がテーブルでない")),
            };
            cur = t
                .entry(key.to_string())
                .or_insert_with(|| toml::Value::Table(toml::map::Map::new()));
        }
        let t = match cur {
            toml::Value::Table(t) => t,
            _ => return Err("TOML のトップレベルがテーブルでない".to_string()),
        };
        let arr_key = last[0];
        t.entry(arr_key.to_string())
            .or_insert_with(|| toml::Value::Array(Vec::new()))
    };
    let arr = match arr {
        toml::Value::Array(a) => a,
        _ => return Err(format!("{} が配列でない", path.join("."))),
    };

    match op {
        "add" => {
            // 重複検査: 同じ id_field のエントリが既にあれば弾く。
            for entry in arr.iter() {
                if let Some(n) = entry.get(id_field).and_then(|v| v.as_str())
                    && n == item
                {
                    return Err(format!(
                        "{} に {id_field}=\"{item}\" のエントリが既に存在する",
                        path.join(".")
                    ));
                }
            }
            let to = to.ok_or("op=add には to (インラインテーブル) が要る")?;
            let entry = parse_inline_entry(to, required)?;
            arr.push(entry);
            Ok(())
        }
        "remove" => {
            let before_len = arr.len();
            arr.retain(|entry| {
                entry
                    .get(id_field)
                    .and_then(|v| v.as_str())
                    .map(|n| n != item)
                    .unwrap_or(true)
            });
            if arr.len() == before_len {
                Err(format!(
                    "{} に {id_field}=\"{item}\" のエントリが見つからない",
                    path.join(".")
                ))
            } else {
                Ok(())
            }
        }
        "replace" => {
            let to = to.ok_or("op=replace には to (インラインテーブル) が要る")?;
            let entry = parse_inline_entry(to, required)?;
            let pos = arr.iter().position(|e| {
                e.get(id_field)
                    .and_then(|v| v.as_str())
                    .map(|n| n == item)
                    .unwrap_or(false)
            });
            match pos {
                Some(i) => {
                    arr[i] = entry;
                    Ok(())
                }
                None => Err(format!(
                    "{} に {id_field}=\"{item}\" のエントリが見つからない",
                    path.join(".")
                )),
            }
        }
        other => Err(format!("未知の op: {other}")),
    }
}

/// TOML スカラ配列を操作する。
/// policy のような Vec<String> フィールドに対して add/remove/replace を行う。
/// item は対象の文字列値そのもの (add 時は to が新要素、remove は item を消す、
/// replace は item を to へ置き換え)。
fn apply_scalar_array(
    doc: &mut toml::Value,
    path: &[&str],
    op: &str,
    item: &str,
    to: Option<&str>,
) -> Result<(), String> {
    let table = match doc {
        toml::Value::Table(t) => t,
        _ => return Err("TOML のトップレベルがテーブルでない".to_string()),
    };
    let arr_key = path[0]; // policy は トップレベル配列のみ対応
    let arr = table
        .entry(arr_key.to_string())
        .or_insert_with(|| toml::Value::Array(Vec::new()));
    let arr = match arr {
        toml::Value::Array(a) => a,
        _ => return Err(format!("{arr_key} が配列でない")),
    };

    match op {
        "add" => {
            let val = to.ok_or("op=add には to (新しい文字列値) が要る")?;
            // 重複検査。
            if arr.iter().any(|v| v.as_str() == Some(val)) {
                return Err(format!("{arr_key} に \"{val}\" が既に存在する"));
            }
            arr.push(toml::Value::String(val.to_string()));
            Ok(())
        }
        "remove" => {
            let before_len = arr.len();
            arr.retain(|v| v.as_str() != Some(item));
            if arr.len() == before_len {
                Err(format!("{arr_key} に \"{item}\" が見つからない"))
            } else {
                Ok(())
            }
        }
        "replace" => {
            let val = to.ok_or("op=replace には to (置換後文字列) が要る")?;
            let pos = arr.iter().position(|v| v.as_str() == Some(item));
            match pos {
                Some(i) => {
                    arr[i] = toml::Value::String(val.to_string());
                    Ok(())
                }
                None => Err(format!("{arr_key} に \"{item}\" が見つからない")),
            }
        }
        other => Err(format!("未知の op: {other}")),
    }
}

/// target → 正本ファイル名。
fn target_file(target: &str) -> Option<&'static str> {
    match target {
        "brand" => Some("brand.md"),
        "rules" => Some("rules.md"),
        "practices" => Some("practices.md"),
        "glossary" => Some("glossary.md"),
        "context" => Some("context.md"),
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

// ── context backend ───────────────────────────────────────────────────────

/// context.md へエントリを即時追加する。
///
/// section = スコープ (## 見出しになる)。text = 1 行インラインテーブル
/// `{ kind = "path", read = ["src/**"], note = ["keep it clean"] }` 形式。
/// kind は省略可 (既定 "task")。read / note は配列または単一文字列。
/// 同名スコープが既に存在する場合は重複として弾く。
/// 書く前に Context::from_markdown で再検証する (パース失敗は書かない)。
fn add_to_context(owox_dir: &Path, today: &str, section: Option<&str>, text: &str) -> Envelope {
    let Some(scope) = section.map(str::trim).filter(|s| !s.is_empty()) else {
        return Envelope::failed(
            "context の add は section (スコープ: ## 見出しになる文字列) が要る",
        );
    };

    // インラインテーブルを解釈する。
    let block = match parse_context_inline(scope, text) {
        Ok(b) => b,
        Err(e) => return Envelope::failed(e),
    };

    let path = owox_dir.join("context.md");
    let body = std::fs::read_to_string(&path).unwrap_or_default();

    // 重複スコープ検査: 既存 ## スコープ と一致する見出しがあれば弾く。
    if context_scope_exists(&body, scope) {
        return Envelope::failed(format!(
            "context に \"{}\" スコープが既に存在する: remove してから再追加する",
            scope
        ));
    }

    // 末尾へ新設する。
    let updated = context_append_block(&body, scope, &block);

    // 書く前に再検証する。
    if let Err(e) = crate::model::Context::from_markdown(&updated) {
        return Envelope::failed(format!("context.md への追記がパース検証で失敗: {e}"));
    }

    if let Err(e) = std::fs::write(&path, updated) {
        return Envelope::failed(format!("{} へ書けない: {e}", path.display()));
    }

    let rec = record_decision(
        owox_dir,
        today,
        RecordInput {
            title: format!("Add context entry: {scope}"),
            status: DecisionStatus::Adopted,
            rationale: format!("Added context entry for scope \"{scope}\". {text}"),
            links: DecisionLinks::default(),
            supersedes: Vec::new(),
        },
    );
    Envelope::ok(
        format!("Added context entry for scope \"{scope}\"."),
        serde_json::json!({ "target": "context", "scope": scope }),
    )
    .with_decision_ids(rec.decision_ids)
}

/// context.md の remove / replace を人間ゲートとして記録する。
///
/// item = スコープ見出し (## の後の文字列)。
/// propose 時点でスコープが存在しない場合は fail-fast (利用可能スコープを data で返す)。
fn propose_context(
    owox_dir: &Path,
    today: &str,
    file: &str,
    op: &str,
    input: &ProposeInput,
) -> Envelope {
    if op != "remove" && op != "replace" {
        return Envelope::failed(format!(
            "context の op は remove / replace のいずれか: {op}"
        ));
    }
    let Some(scope) = input.item.map(str::trim).filter(|s| !s.is_empty()) else {
        return Envelope::failed("context の remove / replace は item (スコープ見出し) が要る");
    };
    let to = if op == "replace" {
        match input.to.map(str::trim).filter(|s| !s.is_empty()) {
            Some(t) => {
                // replace の to もインラインテーブルとして事前検証する (ゲート作成前に弾く)。
                if let Err(e) = parse_context_inline(scope, t) {
                    return Envelope::failed(format!("to のパースに失敗: {e}"));
                }
                Some(t.to_string())
            }
            None => return Envelope::failed("op=replace には to (インラインテーブル) が要る"),
        }
    } else {
        None
    };

    let path = owox_dir.join(file);
    let body = std::fs::read_to_string(&path).unwrap_or_default();

    // fail-fast: スコープが存在しない場合は利用可能スコープを返す。
    if !context_scope_exists(&body, scope) {
        let available = context_scopes(&body);
        return Envelope::failed(format!(
            "context に \"{}\" スコープが見つからない。下の scopes から選んで再提案する",
            scope
        ))
        .with_data(serde_json::json!({
            "target": "context",
            "available_scopes": available,
        }));
    }

    let action = match &to {
        Some(to) => format!("Replace context scope \"{scope}\" body with: {to}"),
        None => format!("Remove context scope \"{scope}\""),
    };
    let rec = record_decision_with_change(
        owox_dir,
        today,
        RecordInput {
            title: format!("Propose context change: {scope}"),
            status: DecisionStatus::Open,
            rationale: format!("Proposed change to {file}. {action}"),
            links: DecisionLinks::default(),
            supersedes: Vec::new(),
        },
        Some(ProposedChange {
            target: "context".to_string(),
            // heading を scope として転用する (apply 側で照合に使う)。
            heading: scope.to_string(),
            op: op.to_string(),
            item: scope.to_string(),
            to,
        }),
    );

    Envelope::needs_human(
        format!(
            "Changing the context map is a decision for a human. Recorded an open gate with the exact change ({action}). When the human decides to approve, call gate.approve yourself; its CLI confirmation prompt is the human's approval, and owox then applies the change to {file}. Do not edit {file} yourself."
        ),
        Gate {
            kind: "canon".to_string(),
            subject: format!("context {op}: {scope}"),
            requires: format!(
                "When the human decides, call gate.approve; its CLI confirmation prompt is the human's approval and owox applies the change to {file}."
            ),
        },
    )
    .with_decision_ids(rec.decision_ids)
}

/// context.md の ProposedChange を適用する。
///
/// change.item = スコープ見出し。op = remove / replace。
/// セクションブロック全体 (## <scope> + 箇条書き) を対象にする。
/// 適用前後に Context::from_markdown で再検証し、失敗したら書かない。
fn apply_context_change(owox_dir: &Path, change: &ProposedChange) -> Result<(), String> {
    let path = owox_dir.join("context.md");
    let body = std::fs::read_to_string(&path)
        .map_err(|e| format!("{} を読めない: {e}", path.display()))?;

    let updated = match change.op.as_str() {
        "remove" => remove_context_block(&body, &change.item)?,
        "replace" => {
            let to = change.to.as_deref().ok_or("op=replace には to が要る")?;
            let new_block = parse_context_inline(&change.item, to)
                .map_err(|e| format!("to のパースに失敗: {e}"))?;
            replace_context_block(&body, &change.item, &new_block)?
        }
        other => return Err(format!("未知の context op: {other}")),
    };

    // 再検証。
    crate::model::Context::from_markdown(&updated)
        .map_err(|e| format!("変更後の context.md がパース検証で失敗: {e}"))?;

    std::fs::write(&path, updated).map_err(|e| format!("{} へ書けない: {e}", path.display()))?;
    Ok(())
}

/// 1 行インラインテーブル文字列を解釈して context.md のブロック本文行を生成する。
///
/// 形式: `{ kind = "path", read = ["src/**"], note = ["keep it clean"] }`
/// kind は省略可 (既定 "task")。read / note は配列または単一文字列。
/// 戻り値は箇条書き行の文字列 (改行なし行のベクタ)。
fn parse_context_inline(scope: &str, text: &str) -> Result<Vec<String>, String> {
    // `x = <text>` で包んで TOML パースする (parse_inline_entry と同じ手法)。
    let wrapped = format!("x = {text}\n");
    let parsed: toml::Value = toml::from_str(&wrapped).map_err(|e| {
        format!(
            "text はインラインテーブルで渡す \
             (例: {{ kind = \"path\", read = [\"src/**\"], note = [\"keep it clean\"] }}): {e}"
        )
    })?;
    let entry = parsed.get("x").cloned().ok_or("text の解釈に失敗")?;
    if !entry.is_table() {
        return Err("text はインラインテーブル ({ key = value, ... }) で渡す".to_string());
    }

    // kind: 省略時 "task"。"task" / "path" のみ許可。
    let kind_str = entry.get("kind").and_then(|v| v.as_str()).unwrap_or("task");
    if kind_str != "task" && kind_str != "path" {
        return Err(format!("kind は task / path のみ: {kind_str}"));
    }

    // read: 配列または単一文字列。
    let reads = extract_str_array(&entry, "read", scope)?;
    // note: 配列または単一文字列。
    let notes = extract_str_array(&entry, "note", scope)?;

    // context.md の箇条書き行を組み立てる。
    let mut lines = vec![format!("- kind: {kind_str}")];
    for r in &reads {
        lines.push(format!("- read: {r}"));
    }
    for n in &notes {
        lines.push(format!("- note: {n}"));
    }
    Ok(lines)
}

/// TOML Value のフィールドを文字列配列として取り出す。
/// 配列または単一文字列を受け入れる。フィールドが無ければ空ベクタ。
fn extract_str_array(entry: &toml::Value, key: &str, scope: &str) -> Result<Vec<String>, String> {
    let Some(val) = entry.get(key) else {
        return Ok(Vec::new());
    };
    match val {
        toml::Value::String(s) => Ok(vec![s.clone()]),
        toml::Value::Array(arr) => {
            let mut out = Vec::new();
            for v in arr {
                match v {
                    toml::Value::String(s) => out.push(s.clone()),
                    other => {
                        return Err(format!("{scope}: {key} の要素は文字列のみ: {other:?}"));
                    }
                }
            }
            Ok(out)
        }
        other => Err(format!(
            "{scope}: {key} は文字列または文字列配列で渡す: {other:?}"
        )),
    }
}

/// context.md 本文の末尾へ新しいセクションブロックを追記する。
fn context_append_block(body: &str, scope: &str, lines: &[String]) -> String {
    let mut out = body.to_string();
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    out.push('\n');
    out.push_str(&format!("## {scope}\n"));
    for line in lines {
        out.push_str(line);
        out.push('\n');
    }
    out
}

/// context.md に指定スコープの `## <scope>` ブロックが存在するか。
fn context_scope_exists(body: &str, scope: &str) -> bool {
    let needle = format!("## {scope}");
    body.lines().any(|l| l.trim_end() == needle)
}

/// context.md の全スコープ見出しを返す (available_scopes 開示用)。
fn context_scopes(body: &str) -> Vec<String> {
    body.lines()
        .filter_map(|l| {
            let t = l.trim_end();
            t.strip_prefix("## ").map(|s| s.to_string())
        })
        .collect()
}

/// context.md から `## <scope>` ブロックを削除する。
///
/// スコープが見つからない場合は Err。ブロックは `## scope` 行から次の `## ` 行
/// (または末尾) までの全行。
fn remove_context_block(body: &str, scope: &str) -> Result<String, String> {
    let needle = format!("## {scope}");
    let mut found = false;
    let mut in_block = false;
    let mut out = String::new();
    for line in body.split_inclusive('\n') {
        let t = line.trim_end();
        if t == needle {
            found = true;
            in_block = true;
            continue; // スコープ見出し行を除去する。
        }
        if in_block && t.starts_with("## ") {
            in_block = false;
        }
        if !in_block {
            out.push_str(line);
        }
    }
    if !found {
        return Err(format!(
            "context.md に \"{}\" スコープが見つからない (canon が変わった可能性)",
            scope
        ));
    }
    Ok(out)
}

/// context.md の `## <scope>` ブロック本文を `new_lines` で置き換える。
///
/// スコープ見出し行は保持し、直後の箇条書き行群を new_lines で上書きする。
/// スコープが見つからない場合は Err。
fn replace_context_block(body: &str, scope: &str, new_lines: &[String]) -> Result<String, String> {
    let needle = format!("## {scope}");
    let mut found = false;
    let mut in_block = false;
    let mut replaced = false;
    let mut out = String::new();
    for line in body.split_inclusive('\n') {
        let t = line.trim_end();
        if t == needle {
            found = true;
            in_block = true;
            out.push_str(line); // 見出し行は残す。
            // 新しいブロック本文を挿入する。
            for new_line in new_lines {
                out.push_str(new_line);
                out.push('\n');
            }
            replaced = true;
            continue;
        }
        if in_block && t.starts_with("## ") {
            in_block = false;
        }
        if in_block && replaced {
            // 旧ブロック本文の行はスキップする。
            continue;
        }
        out.push_str(line);
    }
    if !found {
        return Err(format!(
            "context.md に \"{}\" スコープが見つからない (canon が変わった可能性)",
            scope
        ));
    }
    Ok(out)
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
        std::fs::write(owox.join("rules.md"), "## Safety\n\n- never push --force\n").unwrap();
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

    // ── structured backend (config.toml) ─────────────────────────────────

    /// config.toml に最小限の内容を書く。
    fn write_config(owox: &Path, text: &str) {
        std::fs::write(owox.join("config.toml"), text).unwrap();
    }

    /// settings.language の変更提案ヘルパ。
    fn propose_language<'a>(
        target: &'a str,
        op: &'a str,
        item: &'a str,
        to: Option<&'a str>,
    ) -> ProposeInput<'a> {
        ProposeInput {
            target,
            op: Some(op),
            section: Some("settings.language"),
            item: Some(item),
            to,
            change: None,
        }
    }

    #[test]
    fn config_add_is_blocked_in_canon_add() {
        // canon.add は structured target を受けない。
        let owox = tempdir();
        let env = canon_add(&owox, "20260628", "config", None, "language = \"ja\"");
        assert_eq!(env.status, Status::Failed);
        let msg = &env.reason;
        assert!(
            msg.contains("canon.propose"),
            "メッセージが canon.propose へ案内するはず: {msg}"
        );
    }

    #[test]
    fn config_propose_language_add_opens_gate_and_config_unchanged() {
        // propose 後は gate が開くが config.toml は変わらない。
        let owox = tempdir();
        write_config(&owox, "");
        let env = canon_propose(
            &owox,
            "20260628",
            propose_language("config", "add", "language", Some("ja")),
        );
        assert_eq!(
            env.status,
            Status::NeedsHuman,
            "propose は人間ゲートになるはず"
        );
        assert!(env.gate.is_some());
        assert!(!env.decision_ids.is_empty());
        // config.toml はまだ変わっていない。
        let text = std::fs::read_to_string(owox.join("config.toml")).unwrap();
        assert!(
            !text.contains("ja"),
            "propose 直後は config.toml 不変のはず"
        );
    }

    #[test]
    fn config_propose_language_apply_writes_language() {
        // propose → apply の一連。language が config.toml へ書き込まれる。
        let owox = tempdir();
        write_config(&owox, "");
        let env = canon_propose(
            &owox,
            "20260628",
            propose_language("config", "add", "language", Some("ja")),
        );
        assert_eq!(env.status, Status::NeedsHuman);
        let id = &env.decision_ids[0];
        // apply で書き込み。
        apply_pending_canon_change(&owox, id).unwrap();
        let text = std::fs::read_to_string(owox.join("config.toml")).unwrap();
        assert!(
            text.contains("ja"),
            "apply 後は language = \"ja\" が含まれるはず: {text}"
        );
        // スキーマ検証も通ること。
        let settings = crate::model::Settings::from_toml(&text).unwrap();
        assert_eq!(settings.language, Some("ja".to_string()));
    }

    #[test]
    fn config_propose_verify_checks_add_then_apply() {
        // verify.checks エントリの add → apply。to は 1 行インラインテーブル。
        let owox = tempdir();
        write_config(&owox, "");
        let env = canon_propose(
            &owox,
            "20260628",
            ProposeInput {
                target: "config",
                op: Some("add"),
                section: Some("verify.checks"),
                item: Some("build"),
                to: Some("{ name = \"build\", command = \"cargo build\" }"),
                change: None,
            },
        );
        assert_eq!(env.status, Status::NeedsHuman);
        // 永続化した to に改行が混入していないこと (record の行指向永続化を壊さない)。
        let id = &env.decision_ids[0];
        let d = crate::record::load_decision(&owox, id).unwrap();
        let stored_to = d.proposed_change.unwrap().to.unwrap();
        assert!(
            !stored_to.contains('\n'),
            "保存した to に改行が無いこと: {stored_to:?}"
        );
        apply_pending_canon_change(&owox, id).unwrap();
        let text = std::fs::read_to_string(owox.join("config.toml")).unwrap();
        let vc = crate::model::VerifyConfig::from_toml(&text).unwrap();
        assert_eq!(vc.checks.len(), 1);
        assert_eq!(vc.checks[0].name, "build");
        assert_eq!(vc.checks[0].command, "cargo build");
    }

    #[test]
    fn config_verify_checks_remove_via_propose_and_apply() {
        // verify.checks エントリを add → apply して存在を確認し、remove → apply で消える。
        let owox = tempdir();
        write_config(
            &owox,
            "[[verify.checks]]\nname = \"build\"\ncommand = \"cargo build\"\n",
        );
        let env = canon_propose(
            &owox,
            "20260628",
            ProposeInput {
                target: "config",
                op: Some("remove"),
                section: Some("verify.checks"),
                item: Some("build"),
                to: None,
                change: None,
            },
        );
        assert_eq!(env.status, Status::NeedsHuman);
        let id = &env.decision_ids[0];
        apply_pending_canon_change(&owox, id).unwrap();
        let text = std::fs::read_to_string(owox.join("config.toml")).unwrap();
        let vc = crate::model::VerifyConfig::from_toml(&text).unwrap();
        assert_eq!(vc.checks.len(), 0, "remove 後は checks が空のはず");
    }

    #[test]
    fn config_verify_checks_remove_missing_name_fails_with_available() {
        // 存在しない name を remove しようとすると Err で available names が返る。
        let owox = tempdir();
        write_config(
            &owox,
            "[[verify.checks]]\nname = \"build\"\ncommand = \"cargo build\"\n",
        );
        let env = canon_propose(
            &owox,
            "20260628",
            ProposeInput {
                target: "config",
                op: Some("remove"),
                section: Some("verify.checks"),
                item: Some("no-such-check"),
                to: None,
                change: None,
            },
        );
        assert_eq!(env.status, Status::Failed);
        let data = env.data.unwrap();
        let names = &data["available_names"];
        assert!(names.is_array());
        let arr = names.as_array().unwrap();
        assert!(arr.iter().any(|v| v.as_str() == Some("build")));
    }

    #[test]
    fn config_verify_checks_add_duplicate_name_fails() {
        // 同じ name の重複追加は apply 時に Err。to は 1 行インラインテーブル。
        let owox = tempdir();
        write_config(
            &owox,
            "[[verify.checks]]\nname = \"build\"\ncommand = \"cargo build\"\n",
        );
        let env = canon_propose(
            &owox,
            "20260628",
            ProposeInput {
                target: "config",
                op: Some("add"),
                section: Some("verify.checks"),
                item: Some("build"),
                to: Some("{ name = \"build\", command = \"cargo build --release\" }"),
                change: None,
            },
        );
        assert_eq!(env.status, Status::NeedsHuman);
        let id = &env.decision_ids[0];
        // apply は重複で Err。config.toml は変わらない。
        let result = apply_pending_canon_change(&owox, id);
        assert!(result.is_err(), "重複 add は Err のはず");
        // config.toml に "build --release" が含まれないことを確認。
        let text = std::fs::read_to_string(owox.join("config.toml")).unwrap();
        assert!(
            !text.contains("build --release"),
            "Err 後は config.toml 不変のはず"
        );
    }

    #[test]
    fn config_verify_checks_add_malformed_inline_table_fails_at_propose() {
        // command を欠くインラインテーブルは propose 時点で弾く (ゲートを作らない・書かない)。
        let owox = tempdir();
        write_config(&owox, "");
        let env = canon_propose(
            &owox,
            "20260628",
            ProposeInput {
                target: "config",
                op: Some("add"),
                section: Some("verify.checks"),
                item: Some("build"),
                to: Some("{ name = \"build\" }"),
                change: None,
            },
        );
        assert_eq!(env.status, Status::Failed, "command 欠落は失敗のはず");
        assert!(env.decision_ids.is_empty(), "ゲートを作らないはず");
        // config.toml は空のまま。
        let text = std::fs::read_to_string(owox.join("config.toml")).unwrap();
        assert!(text.trim().is_empty(), "失敗時は config.toml 不変のはず");
    }

    #[test]
    fn config_verify_checks_add_item_name_mismatch_fails_at_propose() {
        // item と to の name がずれる add は propose で弾く (ゲートを作らない・書かない)。
        let owox = tempdir();
        write_config(&owox, "");
        let env = canon_propose(
            &owox,
            "20260628",
            ProposeInput {
                target: "config",
                op: Some("add"),
                section: Some("verify.checks"),
                item: Some("build"),
                to: Some("{ name = \"lint\", command = \"cargo clippy\" }"),
                change: None,
            },
        );
        assert_eq!(
            env.status,
            Status::Failed,
            "item と name の不一致は失敗のはず"
        );
        assert!(env.decision_ids.is_empty(), "ゲートを作らないはず");
        let text = std::fs::read_to_string(owox.join("config.toml")).unwrap();
        assert!(text.trim().is_empty(), "失敗時は config.toml 不変のはず");
    }

    #[test]
    fn config_verify_checks_replace_swaps_entry() {
        // replace: 既存 build を別 command のインラインテーブルへ置き換える。
        let owox = tempdir();
        write_config(
            &owox,
            "[[verify.checks]]\nname = \"build\"\ncommand = \"cargo build\"\n",
        );
        let env = canon_propose(
            &owox,
            "20260628",
            ProposeInput {
                target: "config",
                op: Some("replace"),
                section: Some("verify.checks"),
                item: Some("build"),
                to: Some("{ name = \"build\", command = \"cargo build --release\" }"),
                change: None,
            },
        );
        assert_eq!(env.status, Status::NeedsHuman);
        let id = &env.decision_ids[0];
        apply_pending_canon_change(&owox, id).unwrap();
        let text = std::fs::read_to_string(owox.join("config.toml")).unwrap();
        let vc = crate::model::VerifyConfig::from_toml(&text).unwrap();
        assert_eq!(vc.checks.len(), 1);
        assert_eq!(vc.checks[0].command, "cargo build --release");
    }

    #[test]
    fn config_revert_returns_explicit_error() {
        // structured revert は v1 未対応 → Err。
        let owox = tempdir();
        write_config(&owox, "");
        let env = canon_propose(
            &owox,
            "20260628",
            propose_language("config", "add", "language", Some("en")),
        );
        let id = &env.decision_ids[0];
        apply_pending_canon_change(&owox, id).unwrap();
        // revert は明示 Err。
        let result = revert_pending_canon_change(&owox, id);
        assert!(result.is_err(), "structured revert は Err のはず");
        let msg = result.unwrap_err();
        assert!(
            msg.contains("structured revert not supported"),
            "エラーメッセージが期待と異なる: {msg}"
        );
    }

    #[test]
    fn config_propose_missing_op_fails() {
        let owox = tempdir();
        write_config(&owox, "");
        // op 無しの自由文提案は structured target では受けない。
        let env = canon_propose(&owox, "20260628", free("config", "remove language"));
        assert_eq!(env.status, Status::Failed);
    }

    #[test]
    fn config_propose_missing_section_fails() {
        let owox = tempdir();
        write_config(&owox, "");
        let env = canon_propose(
            &owox,
            "20260628",
            ProposeInput {
                target: "config",
                op: Some("add"),
                section: None,
                item: Some("language"),
                to: Some("ja"),
                change: None,
            },
        );
        assert_eq!(env.status, Status::Failed);
    }

    // ── structured backend (quality.toml) ────────────────────────────────

    fn write_quality(owox: &Path, text: &str) {
        std::fs::write(owox.join("quality.toml"), text).unwrap();
    }

    /// quality.toml の変更提案ヘルパ。
    fn propose_quality<'a>(
        heading: &'a str,
        op: &'a str,
        item: &'a str,
        to: Option<&'a str>,
    ) -> ProposeInput<'a> {
        ProposeInput {
            target: "quality",
            op: Some(op),
            section: Some(heading),
            item: Some(item),
            to,
            change: None,
        }
    }

    #[test]
    fn quality_add_is_blocked_in_canon_add() {
        // canon.add は structured target を受けない。
        let owox = tempdir();
        let env = canon_add(
            &owox,
            "20260628",
            "quality",
            None,
            "bulk_delete_threshold = 5",
        );
        assert_eq!(env.status, Status::Failed);
        let msg = &env.reason;
        assert!(
            msg.contains("canon.propose"),
            "メッセージが canon.propose へ案内するはず: {msg}"
        );
    }

    #[test]
    fn quality_layers_add_propose_then_apply() {
        // layers add → apply でエントリが書き込まれ、Quality::from_toml で再検証が通る。
        let owox = tempdir();
        write_quality(&owox, "");
        let env = canon_propose(
            &owox,
            "20260628",
            propose_quality(
                "layers",
                "add",
                "core",
                Some("{ name = \"core\", paths = [\"src/core/**\"], autonomy = \"guarded\" }"),
            ),
        );
        assert_eq!(
            env.status,
            Status::NeedsHuman,
            "propose は人間ゲートになるはず: {:?}",
            env.reason
        );
        assert!(env.gate.is_some());
        // propose 直後は quality.toml 不変。
        let before = std::fs::read_to_string(owox.join("quality.toml")).unwrap();
        assert!(
            before.trim().is_empty(),
            "propose 直後は quality.toml 不変のはず"
        );

        let id = &env.decision_ids[0];
        apply_pending_canon_change(&owox, id).unwrap();
        let text = std::fs::read_to_string(owox.join("quality.toml")).unwrap();
        let q = crate::quality::Quality::from_toml(&text).unwrap();
        assert_eq!(q.layers.len(), 1, "layer が 1 件のはず");
        assert_eq!(q.layers[0].name.as_deref(), Some("core"));
        assert_eq!(q.layers[0].autonomy, crate::quality::Autonomy::Guarded);
    }

    #[test]
    fn quality_layers_remove_by_name() {
        // layers remove: name="app" のエントリを削除する。
        let owox = tempdir();
        write_quality(
            &owox,
            "[[layers]]\nname = \"core\"\npaths = [\"src/core/**\"]\nautonomy = \"guarded\"\n\
             [[layers]]\nname = \"app\"\npaths = [\"src/app/**\"]\nautonomy = \"free\"\n",
        );
        let env = canon_propose(
            &owox,
            "20260628",
            propose_quality("layers", "remove", "app", None),
        );
        assert_eq!(env.status, Status::NeedsHuman);
        let id = &env.decision_ids[0];
        apply_pending_canon_change(&owox, id).unwrap();
        let text = std::fs::read_to_string(owox.join("quality.toml")).unwrap();
        let q = crate::quality::Quality::from_toml(&text).unwrap();
        assert_eq!(q.layers.len(), 1, "remove 後は layers が 1 件のはず");
        assert_eq!(q.layers[0].name.as_deref(), Some("core"));
    }

    #[test]
    fn quality_layers_replace_by_name() {
        // layers replace: name="app" のエントリを autonomy=supervised へ置き換える。
        let owox = tempdir();
        write_quality(
            &owox,
            "[[layers]]\nname = \"app\"\npaths = [\"src/app/**\"]\nautonomy = \"free\"\n",
        );
        let env = canon_propose(
            &owox,
            "20260628",
            propose_quality(
                "layers",
                "replace",
                "app",
                Some("{ name = \"app\", paths = [\"src/app/**\"], autonomy = \"supervised\" }"),
            ),
        );
        assert_eq!(env.status, Status::NeedsHuman);
        let id = &env.decision_ids[0];
        apply_pending_canon_change(&owox, id).unwrap();
        let text = std::fs::read_to_string(owox.join("quality.toml")).unwrap();
        let q = crate::quality::Quality::from_toml(&text).unwrap();
        assert_eq!(q.layers.len(), 1);
        assert_eq!(q.layers[0].autonomy, crate::quality::Autonomy::Supervised);
    }

    #[test]
    fn quality_layers_item_name_mismatch_fails_at_propose() {
        // item と to の name がずれる add は propose で弾く。
        let owox = tempdir();
        write_quality(&owox, "");
        let env = canon_propose(
            &owox,
            "20260628",
            propose_quality(
                "layers",
                "add",
                "core",
                Some("{ name = \"infra\", paths = [\"src/infra/**\"], autonomy = \"free\" }"),
            ),
        );
        assert_eq!(
            env.status,
            Status::Failed,
            "item と name の不一致は失敗のはず"
        );
        assert!(env.decision_ids.is_empty(), "ゲートを作らないはず");
    }

    #[test]
    fn quality_layers_malformed_inline_table_fails_at_propose() {
        // autonomy を欠くインラインテーブルは propose 時点で弾く。
        let owox = tempdir();
        write_quality(&owox, "");
        let env = canon_propose(
            &owox,
            "20260628",
            propose_quality(
                "layers",
                "add",
                "core",
                Some("{ name = \"core\", paths = [\"src/**\"] }"),
            ),
        );
        assert_eq!(env.status, Status::Failed, "autonomy 欠落は失敗のはず");
        assert!(env.decision_ids.is_empty(), "ゲートを作らないはず");
    }

    #[test]
    fn quality_bulk_delete_threshold_set_integer() {
        // bulk_delete_threshold を整数で設定する。
        let owox = tempdir();
        write_quality(&owox, "");
        let env = canon_propose(
            &owox,
            "20260628",
            propose_quality(
                "bulk_delete_threshold",
                "add",
                "bulk_delete_threshold",
                Some("5"),
            ),
        );
        assert_eq!(
            env.status,
            Status::NeedsHuman,
            "propose は人間ゲートになるはず: {:?}",
            env.reason
        );
        let id = &env.decision_ids[0];
        apply_pending_canon_change(&owox, id).unwrap();
        let text = std::fs::read_to_string(owox.join("quality.toml")).unwrap();
        let q = crate::quality::Quality::from_toml(&text).unwrap();
        assert_eq!(
            q.bulk_delete_threshold,
            Some(5),
            "bulk_delete_threshold = 5 のはず"
        );
    }

    #[test]
    fn quality_delivery_always_limit_set_integer() {
        // delivery.always_limit を整数で設定する (2 階層スカラ)。
        let owox = tempdir();
        write_quality(&owox, "");
        let env = canon_propose(
            &owox,
            "20260628",
            propose_quality("delivery.always_limit", "add", "always_limit", Some("5")),
        );
        assert_eq!(env.status, Status::NeedsHuman);
        let id = &env.decision_ids[0];
        apply_pending_canon_change(&owox, id).unwrap();
        let text = std::fs::read_to_string(owox.join("quality.toml")).unwrap();
        let q = crate::quality::Quality::from_toml(&text).unwrap();
        assert_eq!(q.delivery.always_limit, 5);
    }

    #[test]
    fn quality_boundaries_deferred_returns_failed() {
        // boundaries remove は v1 defer → Failed・ゲートを作らない・書き込まない。
        let owox = tempdir();
        write_quality(&owox, "");
        let env = canon_propose(
            &owox,
            "20260628",
            propose_quality("boundaries", "remove", "some-boundary", None),
        );
        assert_eq!(
            env.status,
            Status::Failed,
            "boundaries remove は defer のはず"
        );
        assert!(env.decision_ids.is_empty(), "ゲートを作らないはず");
        assert!(
            env.reason.contains("v1 では未対応"),
            "defer メッセージが要る: {}",
            env.reason
        );
    }

    #[test]
    fn quality_budgets_deferred_returns_failed() {
        // budgets add は v1 defer → Failed。
        let owox = tempdir();
        write_quality(&owox, "");
        let env = canon_propose(
            &owox,
            "20260628",
            propose_quality("budgets", "add", "some-budget", Some("{ max_lines = 100 }")),
        );
        assert_eq!(env.status, Status::Failed, "budgets add は defer のはず");
        assert!(env.decision_ids.is_empty(), "ゲートを作らないはず");
    }

    // ── structured backend (release.toml) ────────────────────────────────

    fn write_release(owox: &Path, text: &str) {
        std::fs::write(owox.join("release.toml"), text).unwrap();
    }

    fn propose_release<'a>(
        heading: &'a str,
        op: &'a str,
        item: &'a str,
        to: Option<&'a str>,
    ) -> ProposeInput<'a> {
        ProposeInput {
            target: "release",
            op: Some(op),
            section: Some(heading),
            item: Some(item),
            to,
            change: None,
        }
    }

    #[test]
    fn release_add_is_blocked_in_canon_add() {
        let owox = tempdir();
        let env = canon_add(&owox, "20260628", "release", None, "policy = []");
        assert_eq!(env.status, Status::Failed);
        assert!(env.reason.contains("canon.propose"), "{}", env.reason);
    }

    #[test]
    fn release_checks_add_then_apply() {
        // release.toml の checks add → apply。
        let owox = tempdir();
        write_release(&owox, "");
        let env = canon_propose(
            &owox,
            "20260628",
            propose_release(
                "checks",
                "add",
                "sha256",
                Some("{ name = \"sha256\", command = \"sha256sum -c SHA256SUMS\" }"),
            ),
        );
        assert_eq!(env.status, Status::NeedsHuman);
        let id = &env.decision_ids[0];
        apply_pending_canon_change(&owox, id).unwrap();
        let text = std::fs::read_to_string(owox.join("release.toml")).unwrap();
        let r = crate::release::Release::from_toml(&text).unwrap();
        assert_eq!(r.checks.len(), 1);
        assert_eq!(r.checks[0].name, "sha256");
        assert_eq!(r.checks[0].command, "sha256sum -c SHA256SUMS");
    }

    #[test]
    fn release_artifacts_add_then_apply() {
        // release.toml の artifacts add → apply。識別子は name フィールド (ArtifactRaw { name })。
        let owox = tempdir();
        write_release(&owox, "");
        let env = canon_propose(
            &owox,
            "20260628",
            propose_release(
                "artifacts",
                "add",
                "owox-x86_64-linux.tar.gz",
                Some("{ name = \"owox-x86_64-linux.tar.gz\" }"),
            ),
        );
        assert_eq!(
            env.status,
            Status::NeedsHuman,
            "propose は人間ゲートになるはず: {:?}",
            env.reason
        );
        let id = &env.decision_ids[0];
        apply_pending_canon_change(&owox, id).unwrap();
        let text = std::fs::read_to_string(owox.join("release.toml")).unwrap();
        assert!(
            text.contains("owox-x86_64-linux.tar.gz"),
            "artifacts が書き込まれるはず: {text}"
        );
        let r = crate::release::Release::from_toml(&text).unwrap();
        assert_eq!(r.artifacts.len(), 1);
        assert_eq!(r.artifacts[0], "owox-x86_64-linux.tar.gz");
    }

    #[test]
    fn release_policy_add_then_apply() {
        // release.toml の policy add → apply。
        let owox = tempdir();
        write_release(&owox, "");
        let env = canon_propose(
            &owox,
            "20260628",
            propose_release(
                "policy",
                "add",
                "tag は owox-v<version>",
                Some("tag は owox-v<version>"),
            ),
        );
        assert_eq!(env.status, Status::NeedsHuman);
        let id = &env.decision_ids[0];
        apply_pending_canon_change(&owox, id).unwrap();
        let text = std::fs::read_to_string(owox.join("release.toml")).unwrap();
        let r = crate::release::Release::from_toml(&text).unwrap();
        assert!(r.policy.contains(&"tag は owox-v<version>".to_string()));
    }

    #[test]
    fn release_version_file_set() {
        // release.toml の version.file を設定する (2 階層スカラ)。
        let owox = tempdir();
        write_release(
            &owox,
            "[version]\nfile = \"old.toml\"\npattern = '(?m)^version = \"(.+)\"'\n",
        );
        let env = canon_propose(
            &owox,
            "20260628",
            propose_release("version.file", "replace", "file", Some("Cargo.toml")),
        );
        assert_eq!(env.status, Status::NeedsHuman);
        let id = &env.decision_ids[0];
        apply_pending_canon_change(&owox, id).unwrap();
        let text = std::fs::read_to_string(owox.join("release.toml")).unwrap();
        let r = crate::release::Release::from_toml(&text).unwrap();
        assert_eq!(r.version.as_ref().unwrap().file, "Cargo.toml");
    }

    // ── context backend ──────────────────────────────────────────────────

    fn write_context(owox: &std::path::Path, text: &str) {
        std::fs::write(owox.join("context.md"), text).unwrap();
    }

    #[test]
    fn context_add_immediate_write_and_reparses() {
        // canon_add で context エントリを即時追加し、Context::from_markdown で再読できる。
        let owox = tempdir();
        write_context(&owox, "");
        let env = canon_add(
            &owox,
            "20260628",
            "context",
            Some("core maintenance"),
            "{ kind = \"path\", read = [\"crates/core/**\"], note = [\"keep core deterministic\"] }",
        );
        assert_eq!(env.status, Status::Ok, "add は Ok のはず: {:?}", env.reason);
        assert!(!env.decision_ids.is_empty(), "来歴が記録されるはず");
        // 書き込まれ再読できることを確認する。
        let text = std::fs::read_to_string(owox.join("context.md")).unwrap();
        let ctx = crate::model::Context::from_markdown(&text).expect("context.md がパースできる");
        assert_eq!(ctx.entries.len(), 1, "エントリが 1 件のはず");
        let entry = &ctx.entries[0];
        assert_eq!(entry.scope, "core maintenance");
        assert_eq!(entry.kind, crate::model::ScopeKind::Path);
        assert_eq!(entry.reads, vec!["crates/core/**"]);
        assert_eq!(entry.notes, vec!["keep core deterministic"]);
    }

    #[test]
    fn context_add_duplicate_scope_fails_without_write() {
        // 同名スコープを重複追加すると Failed で context.md は変わらない。
        let owox = tempdir();
        write_context(&owox, "## core maintenance\n- kind: task\n");
        let before = std::fs::read_to_string(owox.join("context.md")).unwrap();
        let env = canon_add(
            &owox,
            "20260628",
            "context",
            Some("core maintenance"),
            "{ kind = \"task\" }",
        );
        assert_eq!(env.status, Status::Failed, "重複は Failed のはず");
        let after = std::fs::read_to_string(owox.join("context.md")).unwrap();
        assert_eq!(before, after, "context.md は変わらないはず");
    }

    #[test]
    fn context_propose_remove_opens_gate_without_writing() {
        // propose remove はゲートを開くが context.md は変わらない。apply で削除する。
        let owox = tempdir();
        write_context(
            &owox,
            "## release work\n- kind: task\n- note: check semver\n",
        );
        let env = canon_propose(
            &owox,
            "20260628",
            ProposeInput {
                target: "context",
                op: Some("remove"),
                section: None,
                item: Some("release work"),
                to: None,
                change: None,
            },
        );
        assert_eq!(
            env.status,
            Status::NeedsHuman,
            "propose remove は人間ゲートのはず"
        );
        assert!(env.gate.is_some());
        // propose 直後は context.md 不変。
        let before = std::fs::read_to_string(owox.join("context.md")).unwrap();
        assert!(
            before.contains("release work"),
            "propose 直後は context.md 不変のはず"
        );
        // apply で削除する。
        let id = &env.decision_ids[0];
        apply_pending_canon_change(&owox, id).unwrap();
        let after = std::fs::read_to_string(owox.join("context.md")).unwrap();
        assert!(
            !after.contains("release work"),
            "apply 後はスコープが消えるはず"
        );
    }

    #[test]
    fn context_propose_replace_then_apply_swaps_body() {
        // propose replace → apply でブロック本文が置き換わる。
        let owox = tempdir();
        write_context(&owox, "## docs work\n- kind: task\n- note: old note\n");
        let env = canon_propose(
            &owox,
            "20260628",
            ProposeInput {
                target: "context",
                op: Some("replace"),
                section: None,
                item: Some("docs work"),
                to: Some("{ kind = \"path\", read = [\"docs/**\"], note = [\"new note\"] }"),
                change: None,
            },
        );
        assert_eq!(
            env.status,
            Status::NeedsHuman,
            "propose replace は人間ゲートのはず"
        );
        let id = &env.decision_ids[0];
        apply_pending_canon_change(&owox, id).unwrap();
        let text = std::fs::read_to_string(owox.join("context.md")).unwrap();
        let ctx = crate::model::Context::from_markdown(&text).expect("再読できるはず");
        assert_eq!(ctx.entries.len(), 1);
        let entry = &ctx.entries[0];
        assert_eq!(entry.scope, "docs work");
        assert_eq!(entry.kind, crate::model::ScopeKind::Path);
        assert_eq!(entry.reads, vec!["docs/**"]);
        assert_eq!(entry.notes, vec!["new note"]);
    }

    #[test]
    fn context_propose_remove_missing_scope_fails_with_available_scopes() {
        // 存在しないスコープへの remove は propose で fail-fast し、
        // 利用可能スコープを data で返す。
        let owox = tempdir();
        write_context(&owox, "## core maintenance\n- kind: task\n");
        let env = canon_propose(
            &owox,
            "20260628",
            ProposeInput {
                target: "context",
                op: Some("remove"),
                section: None,
                item: Some("no such scope"),
                to: None,
                change: None,
            },
        );
        assert_eq!(env.status, Status::Failed, "未知スコープは Failed のはず");
        assert!(env.decision_ids.is_empty(), "ゲートを作らないはず");
        let data = env.data.unwrap();
        let scopes = &data["available_scopes"];
        assert!(scopes.is_array(), "available_scopes が配列のはず");
        let arr = scopes.as_array().unwrap();
        assert!(
            arr.iter().any(|v| v.as_str() == Some("core maintenance")),
            "既存スコープが含まれるはず: {arr:?}"
        );
    }

    #[test]
    fn context_add_malformed_inline_table_fails_without_write() {
        // 不正なインラインテーブルは add で失敗し context.md には書かない。
        let owox = tempdir();
        write_context(&owox, "");
        let env = canon_add(
            &owox,
            "20260628",
            "context",
            Some("bad entry"),
            "not a toml table",
        );
        assert_eq!(env.status, Status::Failed, "不正なテーブルは Failed のはず");
        let text = std::fs::read_to_string(owox.join("context.md")).unwrap();
        assert!(text.trim().is_empty(), "失敗時は context.md を書かないはず");
    }

    // ── structured backend (agents.toml) ────────────────────────────────

    fn write_agents(owox: &std::path::Path, text: &str) {
        std::fs::write(owox.join("agents.toml"), text).unwrap();
    }

    fn propose_agents<'a>(
        heading: &'a str,
        op: &'a str,
        item: &'a str,
        to: Option<&'a str>,
    ) -> ProposeInput<'a> {
        ProposeInput {
            target: "agents",
            op: Some(op),
            section: Some(heading),
            item: Some(item),
            to,
            change: None,
        }
    }

    #[test]
    fn agents_add_blocked_in_canon_add() {
        // canon.add は structured target を受けない。
        let owox = tempdir();
        // structured target は text が空でも is_structured_target で弾かれる前に text.is_empty() が先に弾く。
        // 実際の structured guard を確認するため non-empty text を渡す。
        let env = canon_add(&owox, "20260628", "agents", None, "tier = \"fast\"");
        assert_eq!(env.status, Status::Failed);
        assert!(
            env.reason.contains("canon.propose"),
            "メッセージが canon.propose へ案内するはず: {}",
            env.reason
        );
    }

    #[test]
    fn agents_role_replace_writes_keyed_subtable_and_revalidates() {
        // roles replace: implement の tier を strong へ上書きする。
        // [roles.implement] が書き込まれ、Agents::from_toml で再検証が通る。
        let owox = tempdir();
        write_agents(&owox, "");
        let env = canon_propose(
            &owox,
            "20260628",
            propose_agents(
                "roles",
                "replace",
                "implement",
                Some("{ tier = \"strong\", sandbox = \"workspace-write\" }"),
            ),
        );
        assert_eq!(
            env.status,
            Status::NeedsHuman,
            "propose は人間ゲートのはず: {:?}",
            env.reason
        );
        assert!(env.gate.is_some());
        // propose 直後は agents.toml 不変。
        let before = std::fs::read_to_string(owox.join("agents.toml")).unwrap();
        assert!(
            before.trim().is_empty(),
            "propose 直後は agents.toml 不変のはず"
        );

        let id = &env.decision_ids[0];
        apply_pending_canon_change(&owox, id).unwrap();
        let text = std::fs::read_to_string(owox.join("agents.toml")).unwrap();
        // Agents::from_toml で再検証できること。
        let agents = crate::agents::Agents::from_toml(&text).expect("agents.toml がパースできる");
        let imp = agents.roles.iter().find(|r| r.id == "implement").unwrap();
        assert_eq!(imp.tier, "strong", "tier が strong に変わっているはず");
        // `[roles.implement]` が書き込まれているか確認する。
        assert!(
            text.contains("[roles.implement]") || text.contains("roles"),
            "agents.toml に roles が書かれているはず: {text}"
        );
    }

    #[test]
    fn agents_role_remove_deletes_subtable() {
        // roles remove: [roles.implement] を削除し組込み既定に戻す。
        let owox = tempdir();
        write_agents(&owox, "[roles.implement]\ntier = \"strong\"\n");
        let env = canon_propose(
            &owox,
            "20260628",
            propose_agents("roles", "remove", "implement", None),
        );
        assert_eq!(
            env.status,
            Status::NeedsHuman,
            "propose は人間ゲートのはず: {:?}",
            env.reason
        );
        let id = &env.decision_ids[0];
        apply_pending_canon_change(&owox, id).unwrap();
        let text = std::fs::read_to_string(owox.join("agents.toml")).unwrap();
        // remove 後は [roles.implement] サブテーブルが消えている (or ファイルが空)。
        assert!(
            !text.contains("implement"),
            "remove 後は [roles.implement] が消えているはず: {text}"
        );
        // Agents::from_toml で再検証でき、組込み既定値が返る。
        let agents = crate::agents::Agents::from_toml(&text).expect("agents.toml がパースできる");
        let imp = agents.roles.iter().find(|r| r.id == "implement").unwrap();
        // 組込み既定は balanced。
        assert_eq!(
            imp.tier, "balanced",
            "remove 後は組込み既定 balanced に戻るはず"
        );
    }

    #[test]
    fn agents_role_add_is_rejected() {
        // roles への add は役割5固定のため reject される。
        let owox = tempdir();
        write_agents(&owox, "");
        let env = canon_propose(
            &owox,
            "20260628",
            propose_agents("roles", "add", "executor", Some("{ tier = \"fast\" }")),
        );
        assert_eq!(env.status, Status::Failed, "roles add は Failed のはず");
        assert!(env.decision_ids.is_empty(), "ゲートを作らないはず");
    }

    #[test]
    fn agents_unknown_role_id_rejected_at_propose() {
        // 組込み5役割以外の id は propose で fail-fast される。
        let owox = tempdir();
        write_agents(&owox, "");
        let env = canon_propose(
            &owox,
            "20260628",
            propose_agents(
                "roles",
                "replace",
                "unknown-role",
                Some("{ tier = \"fast\" }"),
            ),
        );
        assert_eq!(env.status, Status::Failed, "未知 role id は Failed のはず");
        assert!(
            env.reason.contains("unknown-role") || env.reason.contains("組込み"),
            "エラーメッセージに未知 id が含まれるはず: {}",
            env.reason
        );
        assert!(env.decision_ids.is_empty(), "ゲートを作らないはず");
    }

    #[test]
    fn agents_variant_add_then_apply() {
        // variants add: 新しい変種を agents.toml へ書き込む。
        let owox = tempdir();
        write_agents(&owox, "");
        let env = canon_propose(
            &owox,
            "20260628",
            propose_agents(
                "variants",
                "add",
                "refute",
                Some(
                    "{ id = \"refute\", applies_to = \"review\", prompt = \"Try to refute.\", tier_override = \"reasoning\" }",
                ),
            ),
        );
        assert_eq!(
            env.status,
            Status::NeedsHuman,
            "propose は人間ゲートのはず: {:?}",
            env.reason
        );
        let id = &env.decision_ids[0];
        apply_pending_canon_change(&owox, id).unwrap();
        let text = std::fs::read_to_string(owox.join("agents.toml")).unwrap();
        let agents = crate::agents::Agents::from_toml(&text).expect("agents.toml がパースできる");
        let v = agents
            .variants
            .iter()
            .find(|v| v.id == "refute")
            .expect("refute が見つかるはず");
        assert_eq!(v.applies_to, "review");
        assert_eq!(v.tier_override.as_deref(), Some("reasoning"));
    }

    #[test]
    fn agents_variant_remove_then_apply() {
        // variants remove: 既存の変種を削除する。
        let owox = tempdir();
        write_agents(
            &owox,
            "[[variants]]\nid = \"security-audit\"\napplies_to = \"review\"\nprompt = \"Focus on security.\"\n",
        );
        // まず from_toml で存在を確認する。
        let text_before = std::fs::read_to_string(owox.join("agents.toml")).unwrap();
        let agents_before = crate::agents::Agents::from_toml(&text_before).unwrap();
        assert!(
            agents_before
                .variants
                .iter()
                .any(|v| v.id == "security-audit")
        );

        let env = canon_propose(
            &owox,
            "20260628",
            propose_agents("variants", "remove", "security-audit", None),
        );
        assert_eq!(
            env.status,
            Status::NeedsHuman,
            "propose は人間ゲートのはず: {:?}",
            env.reason
        );
        let id = &env.decision_ids[0];
        apply_pending_canon_change(&owox, id).unwrap();
        let text = std::fs::read_to_string(owox.join("agents.toml")).unwrap();
        let agents = crate::agents::Agents::from_toml(&text).expect("agents.toml がパースできる");
        assert!(
            !agents
                .variants
                .iter()
                .filter(|v| {
                    // 組込み以外の id だけを確認 (組込み adversarial / gardener は残る)。
                    v.id != "adversarial" && v.id != "gardener"
                })
                .any(|v| v.id == "security-audit"),
            "remove 後は security-audit が消えているはず"
        );
    }

    #[test]
    fn agents_variant_replace_then_apply() {
        // variants replace: 既存の変種のプロンプトを置き換える。
        let owox = tempdir();
        write_agents(
            &owox,
            "[[variants]]\nid = \"security-audit\"\napplies_to = \"review\"\nprompt = \"Old prompt.\"\n",
        );
        let env = canon_propose(
            &owox,
            "20260628",
            propose_agents(
                "variants",
                "replace",
                "security-audit",
                Some(
                    "{ id = \"security-audit\", applies_to = \"review\", prompt = \"New prompt.\" }",
                ),
            ),
        );
        assert_eq!(
            env.status,
            Status::NeedsHuman,
            "propose は人間ゲートのはず: {:?}",
            env.reason
        );
        let id = &env.decision_ids[0];
        apply_pending_canon_change(&owox, id).unwrap();
        let text = std::fs::read_to_string(owox.join("agents.toml")).unwrap();
        let agents = crate::agents::Agents::from_toml(&text).expect("agents.toml がパースできる");
        let v = agents
            .variants
            .iter()
            .find(|v| v.id == "security-audit")
            .expect("security-audit が見つかるはず");
        assert_eq!(v.prompt, "New prompt.", "prompt が置き換わっているはず");
    }

    #[test]
    fn agents_variant_add_with_id_mismatch_fails_at_propose() {
        // to の id が item と一致しない add は propose で弾く。
        let owox = tempdir();
        write_agents(&owox, "");
        let env = canon_propose(
            &owox,
            "20260628",
            propose_agents(
                "variants",
                "add",
                "refute",
                Some("{ id = \"other-id\", applies_to = \"review\", prompt = \"Bad.\" }"),
            ),
        );
        assert_eq!(env.status, Status::Failed, "id 不一致は Failed のはず");
        assert!(env.decision_ids.is_empty(), "ゲートを作らないはず");
        assert!(
            env.reason.contains("一致") || env.reason.contains("id"),
            "エラーメッセージが id 一致を説明するはず: {}",
            env.reason
        );
    }

    #[test]
    fn agents_variant_add_malformed_inline_table_fails_at_propose() {
        // applies_to を欠くインラインテーブルは propose で弾く。
        let owox = tempdir();
        write_agents(&owox, "");
        let env = canon_propose(
            &owox,
            "20260628",
            propose_agents(
                "variants",
                "add",
                "security-audit",
                Some("{ id = \"security-audit\", prompt = \"Missing applies_to.\" }"),
            ),
        );
        assert_eq!(
            env.status,
            Status::Failed,
            "applies_to 欠落は Failed のはず"
        );
        assert!(env.decision_ids.is_empty(), "ゲートを作らないはず");
    }

    #[test]
    fn agents_role_bad_sandbox_value_fails_schema_validation() {
        // sandbox に無効な値を設定すると Agents::from_toml 再検証で失敗し agents.toml は変わらない。
        let owox = tempdir();
        write_agents(&owox, "");
        // propose は通る (インラインテーブル自体は TOML として正しい)。
        let env = canon_propose(
            &owox,
            "20260628",
            propose_agents(
                "roles",
                "replace",
                "implement",
                Some("{ sandbox = \"network-write\" }"),
            ),
        );
        assert_eq!(
            env.status,
            Status::NeedsHuman,
            "propose は通るはず: {:?}",
            env.reason
        );
        let id = &env.decision_ids[0];
        // apply で Agents::from_toml が sandbox 値を弾き Err を返す。agents.toml は変わらない。
        let result = apply_pending_canon_change(&owox, id);
        assert!(result.is_err(), "無効な sandbox 値は apply が Err のはず");
        let err_msg = result.unwrap_err();
        assert!(
            err_msg.contains("スキーマ検証")
                || err_msg.contains("read-only")
                || err_msg.contains("workspace-write"),
            "エラーメッセージが sandbox 検証失敗を説明するはず: {err_msg}"
        );
        // agents.toml は空のまま (書き込まれない)。
        let text = std::fs::read_to_string(owox.join("agents.toml")).unwrap();
        assert!(text.trim().is_empty(), "Err 後は agents.toml 不変のはず");
    }

    #[test]
    fn agents_variant_remove_missing_id_fails_with_available() {
        // 存在しない variants id を remove すると propose で fail-fast し、
        // 利用可能 id を data で返す。
        let owox = tempdir();
        write_agents(&owox, "");
        let env = canon_propose(
            &owox,
            "20260628",
            propose_agents("variants", "remove", "no-such-variant", None),
        );
        assert_eq!(
            env.status,
            Status::Failed,
            "未知 variant id は Failed のはず"
        );
        assert!(env.decision_ids.is_empty(), "ゲートを作らないはず");
        let data = env.data.unwrap();
        let ids = &data["available_ids"];
        assert!(ids.is_array(), "available_ids が配列のはず: {ids:?}");
    }
}
