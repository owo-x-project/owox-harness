//! 記録層: 来歴 (decisions)。`.owox/decisions/<id>.md` に 1 判断 1 ファイル。
//!
//! ID は 日付+slug。中央台帳を持たないのは並行ブランチでの衝突を避けるため
//! (`docs/decisions/20260613-Phase4-tool記録層.md`)。形式は Markdown (prose 正本と一致)。
//! パーサは既存の自作 Markdown パーサを再利用する。
//!
//! 人間ゲートは status=open の来歴として表す。別ストアを持たない。
//! gate.list は open 一覧、gate.approve は open→adopted への遷移 + 承認注記。

use std::path::{Path, PathBuf};

use serde_json::json;

use crate::envelope::Envelope;
use crate::markdown::{Doc, split_pair};
use crate::quality::Autonomy;

/// 来歴の状態。open = まだ人間が決めていない判断点 (= gate)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecisionStatus {
    Open,
    Adopted,
    Rejected,
    Superseded,
}

impl DecisionStatus {
    /// 文字列から状態を読む。mcp の tool 引数の検証にも使う。
    pub fn parse(value: &str) -> Result<DecisionStatus, String> {
        match value.trim() {
            "open" => Ok(DecisionStatus::Open),
            "adopted" => Ok(DecisionStatus::Adopted),
            "rejected" => Ok(DecisionStatus::Rejected),
            "superseded" => Ok(DecisionStatus::Superseded),
            other => Err(format!(
                "status は open / adopted / rejected / superseded のみ: {other}"
            )),
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            DecisionStatus::Open => "open",
            DecisionStatus::Adopted => "adopted",
            DecisionStatus::Rejected => "rejected",
            DecisionStatus::Superseded => "superseded",
        }
    }
}

/// 来歴の link 先。要件・作業・検証へつなぐ。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DecisionLinks {
    pub requirement: Option<String>,
    pub work: Option<String>,
    pub verification: Option<String>,
}

impl DecisionLinks {
    fn is_empty(&self) -> bool {
        self.requirement.is_none() && self.work.is_none() && self.verification.is_none()
    }
}

/// canon ゲートに紐づく具体的な変更。承認 (gate.approve) 時に canon へ適用する。
///
/// canon.propose が構造化された変更 (remove / replace) を受けた時だけ付く。
/// 自由文の提案 (op 無し) には付かず、その場合の解決は従来どおり人間の手編集。
/// 適用の実体は canon 側 (apply_pending_canon_change) が持つ。ここは保存形だけを担う。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProposedChange {
    /// brand / rules / practices / glossary。
    pub target: String,
    /// 対象の見出し (例: Deletion policy / Terms / Practices)。
    pub heading: String,
    /// remove / replace。
    pub op: String,
    /// 一致させる既存項目のテキスト。
    pub item: String,
    /// 置換後テキスト (replace の時だけ)。
    pub to: Option<String>,
}

/// 来歴 1 件。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decision {
    pub id: String,
    pub title: String,
    pub status: DecisionStatus,
    pub rationale: String,
    pub links: DecisionLinks,
    pub supersedes: Vec<String>,
    /// canon ゲートに紐づく具体的な変更 (承認時に canon へ適用する)。
    pub proposed_change: Option<ProposedChange>,
    /// guarded 層のコード操作を承認時に解凍するパス (具体パス・repo 相対)。
    ///
    /// proposed_change が canon 適用を担うのと対称に、これは AI が触りたい guarded 層パスを宣言する。
    /// 人間が gate.approve すると、層の操作前ゲートがこれらのパスへの操作を 1 回だけ通す
    /// (`docs/decisions/20260619-Phase9-guarded承認で解凍.md`)。空なら解凍権限を持たない素の判断点。
    pub authorizes: Vec<String>,
    /// 解凍を一度使い切ったか (one-shot)。authorizes を持つ時だけ意味を持つ。
    pub consumed: bool,
    /// gate.approve が付ける承認注記 (注記 — 日付)。
    pub approval: Option<String>,
    /// owox の自動承認パス (gate.auto_approve) で承認したか。後追い確認の対象になる。
    pub auto_approved: bool,
    /// 自動承認を人間が後から確認済みにしたか。auto_approved の時だけ意味を持つ。
    pub confirmed: bool,
}

/// 来歴記録の入力。decision.record tool が受ける。
#[derive(Debug, Clone)]
pub struct RecordInput {
    pub title: String,
    pub status: DecisionStatus,
    pub rationale: String,
    pub links: DecisionLinks,
    pub supersedes: Vec<String>,
}

impl Decision {
    /// Markdown へ描画する。1 行目の `# title` は人間向けタイトル。
    fn render(&self) -> String {
        let mut out = format!("# {}\n\n", self.title);
        out.push_str(&format!("## Status\n\n{}\n\n", self.status.as_str()));

        if !self.rationale.trim().is_empty() {
            out.push_str(&format!("## Rationale\n\n{}\n\n", self.rationale.trim()));
        }

        if !self.links.is_empty() {
            out.push_str("## Links\n\n");
            if let Some(r) = &self.links.requirement {
                out.push_str(&format!("- requirement: {r}\n"));
            }
            if let Some(w) = &self.links.work {
                out.push_str(&format!("- work: {w}\n"));
            }
            if let Some(v) = &self.links.verification {
                out.push_str(&format!("- verification: {v}\n"));
            }
            out.push('\n');
        }

        if !self.supersedes.is_empty() {
            out.push_str("## Supersedes\n\n");
            for s in &self.supersedes {
                out.push_str(&format!("- {s}\n"));
            }
            out.push('\n');
        }

        if let Some(c) = &self.proposed_change {
            out.push_str("## Proposed change\n\n");
            out.push_str(&format!("- target: {}\n", c.target));
            out.push_str(&format!("- heading: {}\n", c.heading));
            out.push_str(&format!("- op: {}\n", c.op));
            out.push_str(&format!("- item: {}\n", c.item));
            if let Some(to) = &c.to {
                out.push_str(&format!("- to: {to}\n"));
            }
            out.push('\n');
        }

        if !self.authorizes.is_empty() {
            out.push_str("## Authorizes\n\n");
            for path in &self.authorizes {
                out.push_str(&format!("- path: {path}\n"));
            }
            out.push_str(&format!("- consumed: {}\n\n", self.consumed));
        }

        if let Some(note) = &self.approval {
            out.push_str(&format!("## Approval\n\n{note}\n"));
        }

        // 自動承認は後追い確認の対象。確認済みかを構造化して残す (人間の後追い導線が読む)。
        if self.auto_approved {
            out.push_str("\n## Auto approval\n\n");
            out.push_str(&format!("- confirmed: {}\n", self.confirmed));
        }

        out
    }

    /// ファイル本文から読む。title は 1 行目の `# `、他は `## ` 節。
    fn parse(id: &str, text: &str) -> Result<Decision, String> {
        let title = text
            .lines()
            .map(str::trim)
            .find_map(|l| l.strip_prefix("# ").filter(|_| !l.starts_with("## ")))
            .unwrap_or("")
            .trim()
            .to_string();

        let mut doc = Doc::parse(text);

        let status = doc
            .take("Status")
            .map(|s| s.text())
            .ok_or_else(|| "Status セクションが必須".to_string())
            .and_then(|t| DecisionStatus::parse(&t))?;

        let rationale = doc.take("Rationale").map(|s| s.text()).unwrap_or_default();

        let mut links = DecisionLinks::default();
        if let Some(section) = doc.take("Links") {
            for item in section.list() {
                let (key, value) = split_pair(&item);
                match key.as_str() {
                    "requirement" => links.requirement = Some(value),
                    "work" => links.work = Some(value),
                    "verification" => links.verification = Some(value),
                    other => return Err(format!("Links の未知のキー: {other}")),
                }
            }
        }

        let supersedes = doc.take("Supersedes").map(|s| s.list()).unwrap_or_default();

        let mut proposed_change = None;
        if let Some(section) = doc.take("Proposed change") {
            let mut target = String::new();
            let mut heading = String::new();
            let mut op = String::new();
            let mut item = String::new();
            let mut to = None;
            for entry in section.list() {
                let (key, value) = split_pair(&entry);
                match key.as_str() {
                    "target" => target = value,
                    "heading" => heading = value,
                    "op" => op = value,
                    "item" => item = value,
                    "to" => to = Some(value),
                    other => return Err(format!("Proposed change の未知のキー: {other}")),
                }
            }
            proposed_change = Some(ProposedChange {
                target,
                heading,
                op,
                item,
                to,
            });
        }

        let mut authorizes = Vec::new();
        let mut consumed = false;
        if let Some(section) = doc.take("Authorizes") {
            for entry in section.list() {
                let (key, value) = split_pair(&entry);
                match key.as_str() {
                    "path" => authorizes.push(value),
                    "consumed" => consumed = value.trim() == "true",
                    other => return Err(format!("Authorizes の未知のキー: {other}")),
                }
            }
        }

        let approval = doc
            .take("Approval")
            .map(|s| s.text())
            .filter(|t| !t.is_empty());

        let mut auto_approved = false;
        let mut confirmed = false;
        if let Some(section) = doc.take("Auto approval") {
            auto_approved = true;
            for entry in section.list() {
                let (key, value) = split_pair(&entry);
                if key == "confirmed" {
                    confirmed = value.trim() == "true";
                }
            }
        }

        Ok(Decision {
            id: id.to_string(),
            title,
            status,
            rationale,
            links,
            supersedes,
            proposed_change,
            authorizes,
            consumed,
            approval,
            auto_approved,
            confirmed,
        })
    }
}

/// slug の最大長 (char)。ID が肥大すると対話で邪魔になるため上限で切る。
/// 一意性は allocate_id の連番が担うので、切り詰めても衝突しない。
const SLUG_MAX_CHARS: usize = 40;

/// title を slug へ。空白を `-`、ファイル名を壊す文字を除く。多言語 title を許す。
/// 来歴・タスクで共用する (ID は日付+slug の同形式)。長い title は上限で切る。
pub(crate) fn slugify(title: &str) -> String {
    let slug: String = title
        .trim()
        .chars()
        .map(|c| match c {
            // パス区切り・予約文字は `-` に倒す。
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '-',
            c if c.is_whitespace() => '-',
            c => c,
        })
        .collect();
    // 連続・前後の `-` を畳む。
    let collapsed: Vec<&str> = slug.split('-').filter(|s| !s.is_empty()).collect();
    let joined = collapsed.join("-");
    if joined.is_empty() {
        "decision".to_string()
    } else {
        truncate_slug(&joined)
    }
}

/// slug を SLUG_MAX_CHARS 以内へ。語境界 (`-`) で切り、無ければ硬切り。末尾 `-` は残さない。
fn truncate_slug(slug: &str) -> String {
    if slug.chars().count() <= SLUG_MAX_CHARS {
        return slug.to_string();
    }
    let head: String = slug.chars().take(SLUG_MAX_CHARS).collect();
    // 上限内の最後の語境界で切る (`-` は ASCII なのでバイト境界は安全)。
    match head.rfind('-') {
        Some(i) if i > 0 => head[..i].to_string(),
        _ => head,
    }
}

/// 日付プレフィックス `YYYYMMDD-` を剥がす。無ければそのまま返す。
/// 日付つき ID を別 ID の slug へ埋める時に二重日付を防ぐ。
pub(crate) fn strip_date_prefix(id: &str) -> &str {
    if let Some((head, rest)) = id.split_once('-')
        && head.len() == 8
        && head.bytes().all(|b| b.is_ascii_digit())
    {
        return rest;
    }
    id
}

/// `.owox/decisions/`。
fn decisions_dir(owox_dir: &Path) -> PathBuf {
    owox_dir.join("decisions")
}

/// 同日内で衝突しない ID を決める。`<today>-<slug>`、衝突時は `-2`, `-3` …。
/// 来歴・タスクで共用する (それぞれのディレクトリで一意化)。
pub(crate) fn allocate_id(dir: &Path, today: &str, slug: &str) -> String {
    let base = format!("{today}-{slug}");
    if !dir.join(format!("{base}.md")).exists() {
        return base;
    }
    let mut n = 2;
    loop {
        let candidate = format!("{base}-{n}");
        if !dir.join(format!("{candidate}.md")).exists() {
            return candidate;
        }
        n += 1;
    }
}

/// 全来歴を読む。`.owox/decisions/*.md`。ディレクトリが無ければ空。
pub fn list_decisions(owox_dir: &Path) -> Result<Vec<Decision>, String> {
    let dir = decisions_dir(owox_dir);
    let read = match std::fs::read_dir(&dir) {
        Ok(read) => read,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(format!("{} を読めない: {err}", dir.display())),
    };

    let mut decisions = Vec::new();
    for entry in read {
        let path = entry.map_err(|e| e.to_string())?.path();
        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        let id = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or_default()
            .to_string();
        let text = std::fs::read_to_string(&path)
            .map_err(|e| format!("{} を読めない: {e}", path.display()))?;
        let decision = Decision::parse(&id, &text)
            .map_err(|e| format!("{} を解釈できない: {e}", path.display()))?;
        decisions.push(decision);
    }
    // ID (日付+slug) で時系列に並べる。
    decisions.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(decisions)
}

/// decision.record。来歴を `.owox/decisions/<id>.md` へ書き、封筒で返す。
///
/// today は呼び出し側 (mcp) が与える `YYYYMMDD` (core は時計を読まない)。
pub fn record_decision(owox_dir: &Path, today: &str, input: RecordInput) -> Envelope {
    record_decision_full(owox_dir, today, input, None, Vec::new())
}

/// 承認時に canon へ適用する具体変更を紐づけて open ゲートを記録する。
/// canon.propose の構造化変更 (remove / replace) 専用。他は record_decision を使う。
pub fn record_decision_with_change(
    owox_dir: &Path,
    today: &str,
    input: RecordInput,
    proposed_change: Option<ProposedChange>,
) -> Envelope {
    record_decision_full(owox_dir, today, input, proposed_change, Vec::new())
}

/// guarded 層のコード操作を承認時に解凍するパスを紐づけて open ゲートを記録する。
/// 層の操作前ゲートで止まった AI が、触りたいパスを宣言して人間承認へ回す入口
/// (`docs/decisions/20260619-Phase9-guarded承認で解凍.md`)。
pub fn record_decision_with_authorization(
    owox_dir: &Path,
    today: &str,
    input: RecordInput,
    authorizes: Vec<String>,
) -> Envelope {
    record_decision_full(owox_dir, today, input, None, authorizes)
}

/// open ゲート (来歴) を 1 件記録する共通経路。canon 適用 (proposed_change)・コード解凍 (authorizes)
/// の有無を引数で受け、構築を 1 箇所に集約する。
fn record_decision_full(
    owox_dir: &Path,
    today: &str,
    input: RecordInput,
    proposed_change: Option<ProposedChange>,
    authorizes: Vec<String>,
) -> Envelope {
    if input.title.trim().is_empty() {
        return Envelope::failed("title が空");
    }

    let dir = decisions_dir(owox_dir);
    if let Err(err) = std::fs::create_dir_all(&dir) {
        return Envelope::failed(format!("{} を作れない: {err}", dir.display()));
    }

    let id = allocate_id(&dir, today, &slugify(&input.title));
    let decision = Decision {
        id: id.clone(),
        title: input.title,
        status: input.status,
        rationale: input.rationale,
        links: input.links,
        supersedes: input.supersedes,
        proposed_change,
        authorizes,
        consumed: false,
        approval: None,
        auto_approved: false,
        confirmed: false,
    };

    let path = dir.join(format!("{id}.md"));
    if let Err(err) = std::fs::write(&path, decision.render()) {
        return Envelope::failed(format!("{} へ書けない: {err}", path.display()));
    }

    let reason = if decision.status == DecisionStatus::Open {
        "Recorded an open decision. It is a pending human gate until approved."
    } else {
        "Recorded the decision."
    };
    Envelope::ok(reason, json!({ "id": id })).with_decision_ids(vec![id])
}

/// 来歴を 1 件読む。canon の承認適用 (apply_pending_canon_change) が
/// 紐づく proposed_change を参照するのに使う。
pub fn load_decision(owox_dir: &Path, id: &str) -> Result<Decision, String> {
    let path = decisions_dir(owox_dir).join(format!("{id}.md"));
    let text = std::fs::read_to_string(&path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            format!("来歴が無い: {id}")
        } else {
            format!("{} を読めない: {e}", path.display())
        }
    })?;
    Decision::parse(id, &text)
}

/// gate.list。status=open の来歴 (未承認の判断点) を返す。
pub fn list_gates(owox_dir: &Path) -> Envelope {
    let decisions = match list_decisions(owox_dir) {
        Ok(d) => d,
        Err(err) => return Envelope::failed(err),
    };
    let pending: Vec<_> = decisions
        .iter()
        .filter(|d| d.status == DecisionStatus::Open)
        .map(|d| json!({ "id": d.id, "title": d.title, "subject": d.rationale }))
        .collect();
    Envelope::ok(
        format!("{} pending gate(s).", pending.len()),
        json!({ "pending": pending }),
    )
}

/// gate.approve。open の来歴を adopted へ遷移し、承認注記を残す。
pub fn approve_gate(owox_dir: &Path, today: &str, id: &str, note: Option<&str>) -> Envelope {
    let path = decisions_dir(owox_dir).join(format!("{id}.md"));
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return Envelope::failed(format!("来歴が無い: {id}"));
        }
        Err(err) => return Envelope::failed(format!("{} を読めない: {err}", path.display())),
    };

    let mut decision = match Decision::parse(id, &text) {
        Ok(d) => d,
        Err(err) => return Envelope::failed(err),
    };

    if decision.status != DecisionStatus::Open {
        return Envelope::failed(format!(
            "来歴 {id} は open ではない (現在 {})。承認できる判断点ではない",
            decision.status.as_str()
        ));
    }

    decision.status = DecisionStatus::Adopted;
    let note_text = note.unwrap_or("Approved by human.");
    decision.approval = Some(format!("{note_text} — {today}"));

    if let Err(err) = std::fs::write(&path, decision.render()) {
        return Envelope::failed(format!("{} へ書けない: {err}", path.display()));
    }

    Envelope::ok(
        "Gate approved. The decision is now adopted.",
        json!({ "id": id, "approved": true }),
    )
    .with_decision_ids(vec![id.to_string()])
}

/// ゲートの自律度。owox の自動承認パス (gate.auto_approve) が「人間のみか auto 可か」を決める。
///
/// 既存の自律度勾配 (`docs/decisions/20260619-承認と自動改善ループ.md`) に乗せる:
/// 成長層 (practices) の canon 変更は Supervised で auto 可。固定層 (brand/rules/glossary) の
/// 変更と、紐づく変更を持たない素の open 判断 (人間判断を要すと AI 自身が記録したもの) は Guarded。
pub fn gate_autonomy(decision: &Decision) -> Autonomy {
    // コード解凍ゲート (authorizes 持ち) は常に人間のみ。auto 承認で guarded 層を解凍させない。
    if !decision.authorizes.is_empty() {
        return Autonomy::Guarded;
    }
    match decision.proposed_change.as_ref().map(|c| c.target.as_str()) {
        Some("practices") => Autonomy::Supervised,
        _ => Autonomy::Guarded,
    }
}

/// 解凍ゲートを使い切り (consumed) にする。one-shot。層の操作前ゲートが解凍で通した直後に呼ぶ。
/// adopted のままにし、consumed 印だけ立てる (証跡は残す)。読めない・書けない時は Err。
pub fn mark_gate_consumed(owox_dir: &Path, id: &str) -> Result<(), String> {
    let mut decision = load_decision(owox_dir, id)?;
    if decision.consumed {
        return Ok(());
    }
    decision.consumed = true;
    let path = decisions_dir(owox_dir).join(format!("{id}.md"));
    std::fs::write(&path, decision.render())
        .map_err(|err| format!("{} へ書けない: {err}", path.display()))
}

/// gate.auto_approve の本体。open の来歴を auto 承認印つきで adopted へ遷移する。
///
/// 自律度の判定 (Guarded なら拒否) と auto 窓の有無は呼び出し側 (mcp) が先に確かめる。
/// canon 変更の適用も呼び出し側が合成する (`approve_gate` と同じく循環依存回避のため)。
pub fn approve_gate_auto(owox_dir: &Path, today: &str, id: &str) -> Envelope {
    let mut decision = match load_decision(owox_dir, id) {
        Ok(d) => d,
        Err(err) => return Envelope::failed(err),
    };
    if decision.status != DecisionStatus::Open {
        return Envelope::failed(format!(
            "来歴 {id} は open ではない (現在 {})。承認できる判断点ではない",
            decision.status.as_str()
        ));
    }
    if gate_autonomy(&decision) == Autonomy::Guarded {
        return Envelope::failed(format!(
            "来歴 {id} は人間のみが承認できる (guarded)。auto 承認できない。gate.approve を使う"
        ));
    }

    decision.status = DecisionStatus::Adopted;
    decision.approval = Some(format!("Auto-approved by owox. — {today}"));
    decision.auto_approved = true;
    decision.confirmed = false;

    let path = decisions_dir(owox_dir).join(format!("{id}.md"));
    if let Err(err) = std::fs::write(&path, decision.render()) {
        return Envelope::failed(format!("{} へ書けない: {err}", path.display()));
    }
    Envelope::ok(
        "Gate auto-approved. It is queued for the human to confirm or revert.",
        json!({ "id": id, "approved": true, "auto": true }),
    )
    .with_decision_ids(vec![id.to_string()])
}

/// 自動承認で未確認の来歴 (後追いキュー)。adopted かつ auto_approved かつ未 confirmed。
pub fn list_auto_pending(owox_dir: &Path) -> Result<Vec<Decision>, String> {
    Ok(list_decisions(owox_dir)?
        .into_iter()
        .filter(|d| {
            d.status == DecisionStatus::Adopted && d.auto_approved && !d.confirmed
        })
        .collect())
}

/// gate.confirm。auto 承認を人間が後から確認済みにする。後追いキューから外れる。
pub fn confirm_auto_approval(owox_dir: &Path, today: &str, id: &str) -> Envelope {
    let mut decision = match load_decision(owox_dir, id) {
        Ok(d) => d,
        Err(err) => return Envelope::failed(err),
    };
    if !decision.auto_approved {
        return Envelope::failed(format!(
            "来歴 {id} は自動承認ではない。確認すべき後追い対象ではない"
        ));
    }
    if decision.confirmed {
        return Envelope::ok("Already confirmed.", json!({ "id": id, "confirmed": true }));
    }
    decision.confirmed = true;
    if let Some(note) = decision.approval.take() {
        decision.approval = Some(format!("{note} Confirmed by human — {today}"));
    }
    let path = decisions_dir(owox_dir).join(format!("{id}.md"));
    if let Err(err) = std::fs::write(&path, decision.render()) {
        return Envelope::failed(format!("{} へ書けない: {err}", path.display()));
    }
    Envelope::ok(
        "Auto-approval confirmed.",
        json!({ "id": id, "confirmed": true }),
    )
    .with_decision_ids(vec![id.to_string()])
}

/// gate.revert の記録側。来歴を rejected へ落とし差し戻し注記を残す。
/// 紐づく canon 変更の逆適用は呼び出し側 (mcp) が合成する (循環依存回避)。
pub fn reject_decision(owox_dir: &Path, today: &str, id: &str, note: Option<&str>) -> Envelope {
    let mut decision = match load_decision(owox_dir, id) {
        Ok(d) => d,
        Err(err) => return Envelope::failed(err),
    };
    decision.status = DecisionStatus::Rejected;
    let note_text = note.unwrap_or("Reverted by human.");
    decision.approval = Some(format!("{note_text} — {today}"));
    let path = decisions_dir(owox_dir).join(format!("{id}.md"));
    if let Err(err) = std::fs::write(&path, decision.render()) {
        return Envelope::failed(format!("{} へ書けない: {err}", path.display()));
    }
    Envelope::ok("Reverted.", json!({ "id": id, "reverted": true }))
        .with_decision_ids(vec![id.to_string()])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(title: &str, status: DecisionStatus) -> RecordInput {
        RecordInput {
            title: title.to_string(),
            status,
            rationale: "because".to_string(),
            links: DecisionLinks::default(),
            supersedes: Vec::new(),
        }
    }

    #[test]
    fn slugify_handles_spaces_and_reserved_chars() {
        assert_eq!(slugify("Reversibility boundary"), "Reversibility-boundary");
        assert_eq!(slugify("a/b: c"), "a-b-c");
        assert_eq!(slugify("可逆性 境界"), "可逆性-境界");
        assert_eq!(slugify("   "), "decision");
    }

    #[test]
    fn slugify_truncates_long_titles_at_word_boundary() {
        // 上限超えは語境界 (`-`) で切り、末尾 `-` を残さない。
        let title = "Update requirement login: the statement and many more words here too";
        let slug = slugify(title);
        assert!(slug.chars().count() <= SLUG_MAX_CHARS);
        assert!(!slug.ends_with('-'));
        // 先頭の語は保たれる。
        assert!(slug.starts_with("Update-requirement-login"));
    }

    #[test]
    fn strip_date_prefix_removes_yyyymmdd() {
        // 日付つき ID は日付を剥がし、無い ID はそのまま。
        assert_eq!(
            strip_date_prefix("20260614-有効な認証情報でログインできる"),
            "有効な認証情報でログインできる"
        );
        assert_eq!(strip_date_prefix("plain-title"), "plain-title");
        assert_eq!(strip_date_prefix("2026-short"), "2026-short");
    }

    #[test]
    fn record_then_list_roundtrips() {
        let dir = tempdir();
        let env = record_decision(
            &dir,
            "20260613",
            input("Reversibility boundary", DecisionStatus::Open),
        );
        assert_eq!(env.status, crate::envelope::Status::Ok);

        let decisions = list_decisions(&dir).unwrap();
        assert_eq!(decisions.len(), 1);
        let d = &decisions[0];
        assert_eq!(d.id, "20260613-Reversibility-boundary");
        assert_eq!(d.title, "Reversibility boundary");
        assert_eq!(d.status, DecisionStatus::Open);
        assert_eq!(d.rationale, "because");
    }

    #[test]
    fn same_slug_same_day_gets_suffix() {
        let dir = tempdir();
        record_decision(&dir, "20260613", input("dup", DecisionStatus::Adopted));
        record_decision(&dir, "20260613", input("dup", DecisionStatus::Adopted));
        let ids: Vec<_> = list_decisions(&dir)
            .unwrap()
            .into_iter()
            .map(|d| d.id)
            .collect();
        assert!(ids.contains(&"20260613-dup".to_string()));
        assert!(ids.contains(&"20260613-dup-2".to_string()));
    }

    #[test]
    fn list_gates_shows_only_open() {
        let dir = tempdir();
        record_decision(&dir, "20260613", input("open one", DecisionStatus::Open));
        record_decision(&dir, "20260613", input("done one", DecisionStatus::Adopted));
        let env = list_gates(&dir);
        let data = env.data.unwrap();
        let pending = data["pending"].as_array().unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0]["title"], "open one");
    }

    #[test]
    fn approve_transitions_open_to_adopted() {
        let dir = tempdir();
        let env = record_decision(&dir, "20260613", input("gate me", DecisionStatus::Open));
        let id = env.data.unwrap()["id"].as_str().unwrap().to_string();

        let approved = approve_gate(&dir, "20260614", &id, Some("ok by lead"));
        assert_eq!(approved.status, crate::envelope::Status::Ok);

        let d = &list_decisions(&dir).unwrap()[0];
        assert_eq!(d.status, DecisionStatus::Adopted);
        assert!(d.approval.as_ref().unwrap().contains("ok by lead"));
        assert!(d.approval.as_ref().unwrap().contains("20260614"));
    }

    #[test]
    fn approve_non_open_fails() {
        let dir = tempdir();
        let env = record_decision(&dir, "20260613", input("already", DecisionStatus::Adopted));
        let id = env.data.unwrap()["id"].as_str().unwrap().to_string();
        let r = approve_gate(&dir, "20260614", &id, None);
        assert_eq!(r.status, crate::envelope::Status::Failed);
    }

    #[test]
    fn approve_missing_fails() {
        let dir = tempdir();
        let r = approve_gate(&dir, "20260614", "20260613-nope", None);
        assert_eq!(r.status, crate::envelope::Status::Failed);
    }

    #[test]
    fn authorization_round_trips_and_is_guarded() {
        let dir = tempdir();
        let env = record_decision_with_authorization(
            &dir,
            "20260619",
            input("touch core", DecisionStatus::Open),
            vec!["src/core/a.rs".to_string(), "src/core/b.rs".to_string()],
        );
        let id = env.data.unwrap()["id"].as_str().unwrap().to_string();
        let d = load_decision(&dir, &id).unwrap();
        // render→parse 往復で authorizes/consumed が保たれる。
        assert_eq!(d.authorizes, vec!["src/core/a.rs", "src/core/b.rs"]);
        assert!(!d.consumed);
        // 解凍ゲートは常に Guarded = auto 承認できない (自己承認で核を解凍させない)。
        assert_eq!(gate_autonomy(&d), Autonomy::Guarded);
        assert_eq!(
            approve_gate_auto(&dir, "20260619", &id).status,
            crate::envelope::Status::Failed
        );
    }

    #[test]
    fn mark_gate_consumed_sets_flag_once() {
        let dir = tempdir();
        let env = record_decision_with_authorization(
            &dir,
            "20260619",
            input("touch core", DecisionStatus::Open),
            vec!["src/core/a.rs".to_string()],
        );
        let id = env.data.unwrap()["id"].as_str().unwrap().to_string();
        approve_gate(&dir, "20260619", &id, None);
        mark_gate_consumed(&dir, &id).unwrap();
        let d = load_decision(&dir, &id).unwrap();
        assert!(d.consumed);
        // adopted のまま (証跡を残す)。
        assert_eq!(d.status, DecisionStatus::Adopted);
        // 二度目は no-op。
        assert!(mark_gate_consumed(&dir, &id).is_ok());
    }

    /// proposed_change つきの open gate を 1 件作り ID を返す。
    fn open_change_gate(dir: &Path, target: &str) -> String {
        let env = record_decision_with_change(
            dir,
            "20260619",
            input("Propose", DecisionStatus::Open),
            Some(ProposedChange {
                target: target.to_string(),
                heading: "Practices".to_string(),
                op: "add".to_string(),
                item: "do the thing".to_string(),
                to: None,
            }),
        );
        env.data.unwrap()["id"].as_str().unwrap().to_string()
    }

    #[test]
    fn gate_autonomy_practices_is_supervised_else_guarded() {
        let dir = tempdir();
        let practices = load_decision(&dir, &open_change_gate(&dir, "practices")).unwrap();
        assert_eq!(gate_autonomy(&practices), Autonomy::Supervised);
        let rules = load_decision(&dir, &open_change_gate(&dir, "rules")).unwrap();
        assert_eq!(gate_autonomy(&rules), Autonomy::Guarded);
        // 紐づく変更が無い素の open は人間のみ (guarded)。
        let env = record_decision(&dir, "20260619", input("plain", DecisionStatus::Open));
        let plain = load_decision(&dir, env.data.unwrap()["id"].as_str().unwrap()).unwrap();
        assert_eq!(gate_autonomy(&plain), Autonomy::Guarded);
    }

    #[test]
    fn auto_approve_refuses_guarded() {
        let dir = tempdir();
        let id = open_change_gate(&dir, "rules");
        let r = approve_gate_auto(&dir, "20260619", &id);
        assert_eq!(r.status, crate::envelope::Status::Failed);
        // 拒否されたので open のまま。
        assert_eq!(
            load_decision(&dir, &id).unwrap().status,
            DecisionStatus::Open
        );
    }

    #[test]
    fn auto_approve_supervised_marks_and_queues_and_roundtrips() {
        let dir = tempdir();
        let id = open_change_gate(&dir, "practices");
        let r = approve_gate_auto(&dir, "20260619", &id);
        assert_eq!(r.status, crate::envelope::Status::Ok);
        // ファイル往復で auto_approved/confirmed が保たれる。
        let d = load_decision(&dir, &id).unwrap();
        assert_eq!(d.status, DecisionStatus::Adopted);
        assert!(d.auto_approved);
        assert!(!d.confirmed);
        // 後追いキューに 1 件。
        assert_eq!(list_auto_pending(&dir).unwrap().len(), 1);

        // 確認するとキューから外れる。
        let c = confirm_auto_approval(&dir, "20260620", &id);
        assert_eq!(c.status, crate::envelope::Status::Ok);
        assert!(load_decision(&dir, &id).unwrap().confirmed);
        assert!(list_auto_pending(&dir).unwrap().is_empty());
    }

    #[test]
    fn confirm_rejects_non_auto() {
        let dir = tempdir();
        let env = record_decision(&dir, "20260619", input("plain", DecisionStatus::Adopted));
        let id = env.data.unwrap()["id"].as_str().unwrap().to_string();
        let c = confirm_auto_approval(&dir, "20260620", &id);
        assert_eq!(c.status, crate::envelope::Status::Failed);
    }

    #[test]
    fn reject_sets_rejected_and_clears_queue() {
        let dir = tempdir();
        let id = open_change_gate(&dir, "practices");
        approve_gate_auto(&dir, "20260619", &id);
        let r = reject_decision(&dir, "20260620", &id, None);
        assert_eq!(r.status, crate::envelope::Status::Ok);
        assert_eq!(
            load_decision(&dir, &id).unwrap().status,
            DecisionStatus::Rejected
        );
        // rejected はキューに残らない。
        assert!(list_auto_pending(&dir).unwrap().is_empty());
    }

    /// テスト用の一意な一時ディレクトリ (依存を足さず std だけで作る)。
    fn tempdir() -> PathBuf {
        use std::sync::atomic::{AtomicU32, Ordering};
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let pid = std::process::id();
        let dir = std::env::temp_dir().join(format!("owox-record-test-{pid}-{n}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }
}
