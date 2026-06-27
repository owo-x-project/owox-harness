//! hook 補助ロジック。owox 実行ファイルの `hook` サブコマンドが呼ぶ。
//!
//! 決定論・型付きロジックは core に集め、生成した hook シェルへ散らさない。
//! 出力ラベルは英語固定 (owox-harness は i18n しない)。

use std::path::Path;

use crate::model::{Brand, Canon, Irreversible, Phase, Rules};
use crate::quality::Quality;

/// 床コンテキスト。session 開始と圧縮後に AI へ注入する常時の地図。
///
/// Codex hook の `SessionStart` と `PostCompact` が stdout の `additionalContext` で取り込む。
/// AGENTS.md は廃止し、向き付け・意図ルーティング・Vision・全体スタイルもここへ寄せた
/// (`docs/handoff/20260616-コンテキスト配信の再設計.md`)。圧縮で床が消えても PostCompact が戻す。
///
/// ここに載せるのは「消えにくく持つべき床」のみ。rules 本文と brand のリスト
/// (values/principles/non-goals/success-criteria) は載せず、関連語が出た時の
/// オンデマンド注入 (`policy_injection`) と lookup へ寄せる (最小コンテキスト)。
/// 全体スタイルは作業全般に効くため常時に残す (`docs/decisions/20260612-段階的開示.md`)。
pub fn floor_context(canon: &Canon) -> String {
    let mut out = String::new();
    out.push_str("# Project context\n\n");

    // 応答言語の追従。指示文は英語固定だが、人間への応答だけ正本設定の言語に合わせる
    // (`docs/decisions/20260613-Phase5-実機検証の是正.md`)。未設定なら注入しない。
    if let Some(lang) = &canon.settings.language {
        out.push_str("## Response language\n\n");
        out.push_str(&format!("Respond to the human in {lang}.\n\n"));
    }

    out.push_str("## Vision\n\n");
    out.push_str(&canon.brand.vision);
    out.push_str("\n\n");

    out.push_str("## Project state\n\n");
    out.push_str(&format!("Phase: {}.\n", canon.state.phase.as_str()));
    out.push_str(phase_guidance(canon.state.phase));
    out.push_str("\n\n");

    out.push_str("## Canon\n\n");
    out.push_str(
        "Do not read or edit the canon under .owox/ directly. Use canon.add to add, canon.propose to change or remove, and owox lookup tools to read.\n\n",
    );

    out.push_str("## Entry map\n\n");
    out.push_str("- Use kickoff to orient.\n");
    out.push_str("- Use next to choose work.\n");
    out.push_str("- Use context to find what to read.\n");
    out.push_str("- Use verify before finishing.\n");
    out.push_str("- Use review to inspect changes.\n");
    out.push_str("- Use skill to grow or manage skills.\n");
    out.push_str(
        "- Use rules.lookup, glossary.lookup, and practice.lookup when rules or terms matter.\n",
    );

    out
}

/// SessionStart 床コンテキストの Entry map が宣伝する tool 名の一覧。
///
/// command_routing_findings (庭師の設計時配線検査) はこの一覧を真実源として、
/// commands.toml のいずれかのコマンドから参照されているかを動的に検査する。
/// floor_context の Entry map と必ず同期して更新すること。
pub fn entry_map_tools() -> &'static [&'static str] {
    &[
        "kickoff",
        "next",
        "context",
        "verify",
        "review",
        "skill",
        "rules.lookup",
        "glossary.lookup",
        "practice.lookup",
    ]
}

/// 注入する brand 本文 (`## Brand` ラベル配下)。語トリガ push が使う。
///
/// Vision と全体スタイルは床に常時あるので除き、values/principles/non-goals/success-criteria だけを出す。
/// 出すものが無ければ None (素通り)。
fn render_brand_block(brand: &Brand) -> Option<String> {
    let mut out = String::from("## Brand\n\n");
    let mut any = false;
    for (title, items) in [
        ("Values", &brand.values),
        ("Principles", &brand.principles),
        ("Non-goals", &brand.non_goals),
        ("Success criteria", &brand.success_criteria),
    ] {
        if !items.is_empty() {
            render_sublist(&mut out, title, items);
            any = true;
        }
    }
    any.then_some(out)
}

/// rules に出すべき項目が 1 つでもあるか。
fn has_rules(rules: &Rules) -> bool {
    !rules.entries.is_empty() || !rules.irreversible.is_empty() || !rules.human_gate.is_empty()
}

/// phase でフィルタした時に出すべき項目が 1 つでもあるか。
fn has_rules_for_phase(rules: &Rules, phase: Phase) -> bool {
    rules
        .entries
        .iter()
        .any(|e| e.phase.is_none() || e.phase == Some(phase))
        || !rules.irreversible.is_empty()
        || !rules.human_gate.is_empty()
}

/// entries をセクション別にグループ化して (section, items) のリストを返す。
/// section が空の entries は "Common" 扱いとする。
fn group_entries_by_section(entries: &[crate::model::RuleEntry]) -> Vec<(String, Vec<String>)> {
    // セクション出現順を保ちつつグループ化する。
    let mut order: Vec<String> = Vec::new();
    let mut groups: std::collections::HashMap<String, Vec<String>> =
        std::collections::HashMap::new();
    for e in entries {
        let sec = if e.section.is_empty() {
            "Common".to_string()
        } else {
            e.section.clone()
        };
        if !groups.contains_key(&sec) {
            order.push(sec.clone());
            groups.insert(sec.clone(), Vec::new());
        }
        groups.get_mut(&sec).unwrap().push(e.text.clone());
    }
    order
        .into_iter()
        .map(|sec| {
            let items = groups.remove(&sec).unwrap_or_default();
            (sec, items)
        })
        .collect()
}

/// `- name: detail` を 1 行出す。detail が空なら name だけ。
fn push_pair(out: &mut String, name: &str, detail: &str) {
    out.push_str("- ");
    out.push_str(name);
    if !detail.is_empty() {
        out.push_str(": ");
        out.push_str(detail);
    }
    out.push('\n');
}

/// 全 entries を出す (phase 不問)。`rules.lookup` の旧互換・テスト用。
pub fn render_rules_block(rules: &Rules) -> String {
    if !has_rules(rules) {
        return "## Rules\n\nNo rules are defined for this project yet.\n".to_string();
    }
    let mut out = String::from("## Rules\n\nThe project's rules. Follow them.\n\n");
    for (sec, items) in group_entries_by_section(&rules.entries) {
        render_sublist(&mut out, &sec, &items);
    }
    render_irreversible_and_gates(&mut out, rules);
    out
}

/// 現在 phase (common + 指定 phase) の entries だけを出す。`rules.lookup` と `policy_injection` の真実源。
pub fn render_rules_block_for_phase(rules: &Rules, phase: Phase) -> String {
    if !has_rules_for_phase(rules, phase) {
        return "## Rules\n\nNo rules are defined for this project yet.\n".to_string();
    }
    let filtered: Vec<&crate::model::RuleEntry> = rules
        .entries
        .iter()
        .filter(|e| e.phase.is_none() || e.phase == Some(phase))
        .collect();
    let mut out =
        String::from("## Rules\n\nThe project's rules for the current phase. Follow them.\n\n");

    // セクション別グループ化 (出現順)。
    let mut order: Vec<String> = Vec::new();
    let mut groups: std::collections::HashMap<String, Vec<String>> =
        std::collections::HashMap::new();
    for e in &filtered {
        let sec = if e.section.is_empty() {
            "Common".to_string()
        } else {
            e.section.clone()
        };
        if !groups.contains_key(&sec) {
            order.push(sec.clone());
            groups.insert(sec.clone(), Vec::new());
        }
        groups.get_mut(&sec).unwrap().push(e.text.clone());
    }
    for sec in order {
        let items = groups.remove(&sec).unwrap_or_default();
        render_sublist(&mut out, &sec, &items);
    }

    render_irreversible_and_gates(&mut out, rules);
    out
}

/// Irreversible operations と Human gates を共通レンダリング。
fn render_irreversible_and_gates(out: &mut String, rules: &Rules) {
    if !rules.irreversible.is_empty() {
        out.push_str("### Irreversible operations\n\n");
        out.push_str("Confirm with a human before doing any of these.\n\n");
        for op in &rules.irreversible {
            push_pair(out, &op.operation, &op.reason);
        }
        out.push('\n');
    }
    if !rules.human_gate.is_empty() {
        out.push_str("### Hand back to a human\n\n");
        for gate in &rules.human_gate {
            push_pair(out, &gate.situation, &gate.reason);
        }
        out.push('\n');
    }
}

/// `## title` 配下へ `- item` を並べる。空なら節ごと出さない。
/// `### title` 配下へ `- item` を並べる。`## Rules` / `## Brand` の小節用。空なら出さない。
fn render_sublist(out: &mut String, title: &str, items: &[String]) {
    if items.is_empty() {
        return;
    }
    out.push_str("### ");
    out.push_str(title);
    out.push_str("\n\n");
    for item in items {
        out.push_str("- ");
        out.push_str(item);
        out.push('\n');
    }
    out.push('\n');
}

/// phase に応じた振る舞い案内。session 開始で注入し、AI の動き方を状態へ合わせる
/// (`docs/decisions/20260613-Phase4-tool記録層.md`)。ゲートの厳しさだけでなく、
/// 何を推奨するかも phase で変える。注入文はハーネス標準・英語・1 文。
fn phase_guidance(phase: Phase) -> &'static str {
    match phase {
        Phase::Initial => {
            "The project is in its initial phase. Large, structural refactors are welcome and encouraged to get the design right, so favor cleanliness over preserving current behavior."
        }
        Phase::Stable => {
            "The project is in its stable phase. Balance new work against stability and avoid unnecessary churn."
        }
        Phase::Maintenance => {
            "The project is in its maintenance phase. Keep changes small and reversible, preserve existing behavior, and add a regression test for every fix."
        }
    }
}

/// 用語注入の結果。注入文と、注入した用語名 (小文字化) を返す。
///
/// 用語名は呼び出し側 (mcp) が session 単位で覚え、同じ語を再注入しないために使う
/// (`docs/handoff/20260613-Phase4対話検証で見つけた粗の改善.md`)。
pub struct GlossaryInjection {
    /// AI へ注入する Markdown。
    pub context: String,
    /// 今回注入した用語名 (小文字化)。
    pub terms: Vec<String>,
}

/// `text` に現れた定義済み用語のうち、まだ注入していない語の定義だけを返す (段階的開示の能動 push)。
///
/// 用語名は床コンテキストに常時。意味が要る時、この関数が「text に出た語」の定義だけを
/// 差し込み文へ組む。モデルが探しに行く前に届くので最小コンテキストで効く
/// (`docs/decisions/20260612-段階的開示.md`)。一致ゼロ・全て注入済みなら None (素通り)。
///
/// text はユーザープロンプト (UserPromptSubmit) や編集対象の内容 (PreToolUse の patch) を渡す。
/// `already` は同 session で注入済みの用語名 (小文字化)。同じ語の反復注入を避ける。
/// 照合は大文字小文字を無視した部分一致。用語は多言語・複数語があり正規表現にしにくい。
/// 誤検出は「関連する定義が 1 つ余計に出る」程度で安全側に倒れる。
pub fn glossary_injection(
    canon: &Canon,
    text: &str,
    already: &std::collections::HashSet<String>,
) -> Option<GlossaryInjection> {
    let text_lower = text.to_lowercase();
    let matched: Vec<&crate::model::GlossaryEntry> = canon
        .glossary
        .entries
        .iter()
        .filter(|e| {
            // 別名のいずれかが出ても正規用語の定義を届ける。dedup の鍵は正規用語名へ寄せる。
            e.matches(&text_lower) && !already.contains(&e.term.to_lowercase())
        })
        .collect();

    if matched.is_empty() {
        return None;
    }

    let mut out =
        String::from("# Glossary\n\nDefinitions for project-specific terms that appeared:\n\n");
    let mut terms = Vec::new();
    for entry in matched {
        out.push_str(&format!("- {}: {}\n", entry.term, entry.definition));
        terms.push(entry.term.to_lowercase());
    }
    Some(GlossaryInjection {
        context: out,
        terms,
    })
}

/// rules / brand のオンデマンド注入結果。注入文と、注入した区分キーを返す。
///
/// キーは呼び出し側 (mcp) が session 単位で覚え、同じ区分を再注入しないために使う
/// (glossary 用語と同じ session キャッシュへ載せる。`policy:` 接頭で用語名と衝突しない)。
pub struct PolicyInjection {
    /// AI へ注入する Markdown。
    pub context: String,
    /// 今回注入した区分キー (`policy:rules` / `policy:brand`)。
    pub keys: Vec<String>,
}

/// rules ブロックの区分キー。session キャッシュで重複排除する。用語名はコロンを含まないので衝突しない。
const RULES_KEY: &str = "policy:rules";
/// brand ブロックの区分キー。
const BRAND_KEY: &str = "policy:brand";

/// rules 本文を push する語 (保守的・部分一致。語幹で複数形・派生を拾う)。
const RULES_TRIGGERS: &[&str] = &[
    "rule",
    "polic",
    "delet",
    "remov",
    "depend",
    "safe",
    "irreversible",
    "secret",
    "force push",
    "destructive",
    "rewrite history",
];

/// brand 本文を push する語。value は一般語のため複数形に限り、誤検出を抑える。
const BRAND_TRIGGERS: &[&str] = &[
    "values",
    "principle",
    "non-goal",
    "non goal",
    "nongoal",
    "success criteria",
    "out of scope",
    "brand",
    "identity",
];

/// `text` に rules / brand 関連の語が現れたら、その本文を差し込み文へ組む (段階的開示の能動 push)。
///
/// rules 本文は `## Rules` 配下に集約して届け「rules」と即分かる形にする。brand 本文は
/// values/principles/non-goals/success-criteria を `## Brand` 配下で届ける (Vision・全体スタイルは床に常時)。
/// `force_rules` が true の時は語に関わらず rules を届ける (編集直前に change/safety policy を渡す経路)。
/// `already` は同 session で注入済みの区分キー。同じ区分の反復注入を避ける。
/// 一致ゼロ・全て注入済みなら None (素通り)。照合は大文字小文字を無視した部分一致。
pub fn policy_injection(
    canon: &Canon,
    phase: Phase,
    text: &str,
    already: &std::collections::HashSet<String>,
    force_rules: bool,
) -> Option<PolicyInjection> {
    let lower = text.to_lowercase();
    let mut out = String::new();
    let mut keys = Vec::new();

    let want_rules = force_rules || RULES_TRIGGERS.iter().any(|k| lower.contains(k));
    if want_rules && !already.contains(RULES_KEY) && has_rules_for_phase(&canon.rules, phase) {
        out.push_str(&render_rules_block_for_phase(&canon.rules, phase));
        keys.push(RULES_KEY.to_string());
    }

    let want_brand = BRAND_TRIGGERS.iter().any(|k| lower.contains(k));
    if want_brand
        && !already.contains(BRAND_KEY)
        && let Some(block) = render_brand_block(&canon.brand)
    {
        if !out.is_empty() {
            out.push_str("\n\n");
        }
        out.push_str(&block);
        keys.push(BRAND_KEY.to_string());
    }

    if keys.is_empty() {
        None
    } else {
        Some(PolicyInjection { context: out, keys })
    }
}

/// PreToolUse の判断結果。
pub enum HookDecision {
    /// そのまま許可 (出力無し・終了コード 0 で素通り)。
    Allow,
    /// 不可逆操作として止める。理由を人間・AI へ返す。
    Deny { reason: String },
    /// 止めずに気づきを添える (allow + additionalContext)。
    ///
    /// commit 前の完了確認の誘導など。機械ブロックではない。
    Remind { message: String },
}

/// PreToolUse 時の不可逆判定。
///
/// Bash コマンドが既定の不可逆パターン (∪ target 固有 `detect:`) へ当たれば deny
/// (機械強制)。それ以外は許可。
///
/// git commit の完了ゲートは別 (`commit_gate`)。呼び出し側 (mcp) が不可逆判定の後に
/// `is_git_commit` を見て検査再実行+未承認 gate でゲートする。core を無状態に保つため、
/// 検査実行・来歴読取は呼び出し側で行い、結果をこの core 判定へ渡す
/// (`docs/decisions/20260613-Phase4-tool記録層.md`)。
///
/// `irreversible` は正本 (rules.md) の不可逆操作。owox 同梱の既定検出器に加え、
/// `detect:` を持つ target 固有の不可逆操作も照合する。
pub fn pre_tool_use_decision(
    tool_name: &str,
    command: Option<&str>,
    irreversible: &[Irreversible],
) -> HookDecision {
    if tool_name != "Bash" {
        return HookDecision::Allow;
    }
    let Some(cmd) = command else {
        return HookDecision::Allow;
    };

    if let Some(det) = crate::irreversible::detect_bash(cmd, irreversible) {
        return HookDecision::Deny { reason: det.reason };
    }

    // canon 直読み禁止 (`docs/decisions/20260613-Phase5-スキルと入口.md`)。
    // 指針 canon を Bash で読もうとしたら止め、tool へ誘導する (読みは tool に一本化済み)。
    // `.owox/skills/` は AI の著作作業域なので除外する。
    if reads_canon_guidance(cmd) {
        return HookDecision::Deny {
            reason: "Do not read the project canon under .owox/ directly. Its guidance reaches you through the session context and the owox tools; look up a glossary term with glossary.lookup, and to change or remove canon use canon.propose. You may read and write skills under .owox/skills/.".to_string(),
        };
    }

    HookDecision::Allow
}

/// Bash コマンドが指針 canon (`.owox/` の `.owox/skills/` 以外) を読もうとしているか。
///
/// 内容を読む既知コマンドが在り、かつ canon パスを指す時だけ true (保守的)。
/// `ls .owox` のような列挙や `.owox/skills/` の参照は止めない。完全強制ではなく
/// 指示の補助 (実機 Codex は内蔵閲覧で hook を通らない経路もある)。
fn reads_canon_guidance(command: &str) -> bool {
    const READERS: &[&str] = &[
        "cat", "head", "tail", "less", "more", "bat", "grep", "rg", "sed", "awk", "view", "nl",
        "xxd", "od",
    ];
    let tokens: Vec<&str> = command.split_whitespace().collect();

    let has_reader = tokens.iter().any(|t| {
        let prog = t.trim_start_matches(['|', '(', ';', '&']);
        READERS.contains(&prog)
    });
    if !has_reader {
        return false;
    }

    tokens.iter().any(|t| {
        let p = t.trim_matches(['"', '\'', '`', '(', ')']);
        let refs_owox = p == ".owox" || p.contains(".owox/");
        // `.owox/skills/` は作業域として除外。
        refs_owox && !p.contains(".owox/skills")
    })
}

/// 人間が承認した解凍ゲート 1 件。adopted・未消費の来歴が authorizes に持つパスを呼び出し側が渡す。
pub struct GateAuthorization {
    /// 来歴 ID (解凍に使ったら呼び出し側が consumed にする)。
    pub id: String,
    /// この承認が解凍するパス (来歴の authorizes。相対化前のまま渡してよい)。
    pub paths: Vec<String>,
}

/// 層別操作前ゲートの結果。
pub enum LayerGate {
    /// 層ゲートに引っかからない (素通り)。
    Allow,
    /// guarded 操作だが未承認なので止める。
    Deny { reason: String },
    /// guarded 操作だが承認済み解凍ゲートが覆うので 1 回通す。decision_ids を呼び出し側が consumed にする。
    Thaw { decision_ids: Vec<String> },
}

/// 層別自律度の操作前ゲート (PreToolUse)。
///
/// architecture=layered の時だけ効く (`architecture_layered`)。guarded 層の不可逆/契約面操作を
/// 操作前に人間ゲートへ回す (`docs/decisions/20260618-Phase9-性質軸適応機構.md` の guarded 4段):
/// (2) guarded 層のファイル削除・(3) guarded 層の契約面パス編集。
/// (1) 境界違反は内容依存で commit ゲート (`commit_blocks`)・(4) その他 guarded 編集は素通り (助言は別経路)。
///
/// 操作対象パスは tool 経路ごとに拾う。Bash は削除を rm/git rm から・編集をシェル書き込み
/// (`>` `>>`・tee・sed -i・cp/mv の宛先・dd of=) から、Edit/Write は `file_path` から、
/// apply_patch はパッチ本文 (`command` に入る) の `*** Add/Update/Delete File:` から取る。実機 Codex の
/// 親は削除・編集をすべて apply_patch で送りパスを本文に置き、subagent (worker) は apply_patch を持たず
/// 編集を exec_command (シェル) で行うため、両経路を解析しないと層ゲートが空振りする
/// (`docs/validation/20260620-Phase9-オーケストレーション実機.md` で確証)。パッチや絶対 file_path のパスは
/// `work_dir` 基準で repo 相対へ正規化してから glob 照合する。
///
/// `authorized` は人間が gate.approve 済みの解凍ゲート。guarded 操作対象が全て覆われていれば `Thaw`
/// を返し、呼び出し側がその来歴を消費して操作を 1 回通す (`docs/decisions/20260619-Phase9-guarded承認で解凍.md`)。
/// 1 つでも未承認の guarded 対象があれば従来どおり `Deny`。
pub fn layer_pre_action_gate(
    tool_name: &str,
    command: Option<&str>,
    file_path: Option<&str>,
    quality: &Quality,
    architecture_layered: bool,
    work_dir: &Path,
    authorized: &[GateAuthorization],
) -> LayerGate {
    if !architecture_layered {
        return LayerGate::Allow;
    }

    // 操作対象を「削除」「編集」へ振り分けて集める。経路 (Bash / apply_patch / Edit・Write・MultiEdit)
    // を吸収する。Edit/Write は Codex・Claude Code 共通、MultiEdit は Claude Code 固有の複数編集 tool
    // (どちらも file_path を持つ)。NotebookEdit は notebook_path で入力形が別のため v1 は対象外
    // (`docs/decisions/20260621-Phase9-マルチCLI生成.md`)。
    let mut deletions: Vec<String> = Vec::new();
    let mut edits: Vec<String> = Vec::new();
    match tool_name {
        "Bash" => {
            if let Some(cmd) = command {
                deletions.extend(rm_targets(cmd));
                edits.extend(write_targets(cmd));
            }
        }
        "apply_patch" => {
            if let Some(patch) = command {
                for change in parse_patch_changes(patch) {
                    match change.op {
                        PatchOp::Delete => deletions.push(change.path),
                        PatchOp::Add | PatchOp::Update => edits.push(change.path),
                    }
                }
            }
        }
        "Edit" | "Write" | "MultiEdit" => {
            if let Some(path) = file_path {
                edits.push(path.to_string());
            }
        }
        _ => {}
    }

    // 人間ゲートが要る guarded 操作を (repo 相対パス, 何の承認が要るか) で集める。
    // (2) guarded 層のファイル削除・(3) guarded 層の契約面パス編集。
    let mut needs_approval: Vec<(String, &str)> = Vec::new();
    for target in &deletions {
        let rel = relativize_path(target, work_dir);
        if quality.layer_autonomy(&rel) == crate::quality::Autonomy::Guarded {
            needs_approval.push((rel, "delete"));
        }
    }
    for path in &edits {
        let rel = relativize_path(path, work_dir);
        if quality.is_contract_surface(&rel) {
            needs_approval.push((rel, "edit"));
        }
    }

    if needs_approval.is_empty() {
        return LayerGate::Allow;
    }

    // 全ての guarded 対象が承認済み解凍ゲートで覆われる時だけ通す。覆われない対象は全て集めて
    // 1 つの deny で名指しし、AI が 1 ゲートにまとめて承認へ回せるようにする (承認回数を減らす)。
    let mut to_consume: Vec<String> = Vec::new();
    let mut uncovered: Vec<(String, &str)> = Vec::new();
    for (path, kind) in needs_approval {
        match authorized
            .iter()
            .find(|a| a.paths.iter().any(|p| relativize_path(p, work_dir) == path))
        {
            Some(a) => {
                if !to_consume.contains(&a.id) {
                    to_consume.push(a.id.clone());
                }
            }
            None => uncovered.push((path, kind)),
        }
    }

    if uncovered.is_empty() {
        return LayerGate::Thaw {
            decision_ids: to_consume,
        };
    }

    let clauses: Vec<String> = uncovered
        .iter()
        .map(|(path, kind)| match *kind {
            "delete" => format!("{path} needs approval to delete"),
            _ => format!("{path} is a contract surface that needs approval to edit"),
        })
        .collect();
    LayerGate::Deny {
        reason: format!(
            "These paths are in a guarded layer and need human approval before this change: {}. Record one decision with decision.record that lists all of them under authorizes, then a human approves it with gate.approve before you retry.",
            clauses.join("; ")
        ),
    }
}

/// apply_patch のパッチ本文の 1 操作。層ゲートは path を層 glob へ照合する。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PatchOp {
    Add,
    Update,
    Delete,
}

/// パッチ本文から取り出した 1 変更 (操作 + 対象パス)。
#[derive(Debug, PartialEq, Eq)]
pub struct PatchChange {
    pub op: PatchOp,
    pub path: String,
}

/// apply_patch のパッチ本文から `(操作, パス)` を取り出す。
///
/// 封筒は `*** Begin Patch` … `*** End Patch`、各ファイルは `*** Add/Update/Delete File: <path>`、
/// rename は `*** Move to: <path>` (移動先を Add 扱いにする)。差分本文 (`@@` / `+` / `-`) は見ない。
pub fn parse_patch_changes(patch: &str) -> Vec<PatchChange> {
    // 移動先 (Move to) は Add 扱い。先頭から当たった marker を採る。
    const MARKERS: [(&str, PatchOp); 4] = [
        ("*** Add File: ", PatchOp::Add),
        ("*** Update File: ", PatchOp::Update),
        ("*** Delete File: ", PatchOp::Delete),
        ("*** Move to: ", PatchOp::Add),
    ];
    let mut changes = Vec::new();
    for line in patch.lines() {
        let line = line.trim_end();
        let parsed = MARKERS
            .into_iter()
            .find_map(|(marker, op)| line.strip_prefix(marker).map(|p| (op, p)));
        if let Some((op, path)) = parsed {
            let path = path.trim();
            if !path.is_empty() {
                changes.push(PatchChange {
                    op,
                    path: path.to_string(),
                });
            }
        }
    }
    changes
}

/// 操作対象パスを repo 相対へ正規化する。層 glob (`src/core/**` など) は repo 相対前提なので、
/// apply_patch や絶対パスの file_path をそのまま照合すると当たらない。
///
/// `work_dir` 配下の絶対パスはその接頭を剥がす。既に相対なら先頭 `./` だけ落とす。
/// work_dir 配下でない絶対パスは層に属さないので、そのまま返す (どの glob にも当たらず素通り)。
fn relativize_path(path: &str, work_dir: &Path) -> String {
    if let Ok(stripped) = Path::new(path).strip_prefix(work_dir) {
        return stripped.to_string_lossy().into_owned();
    }
    path.trim_start_matches("./").to_string()
}

/// Bash コマンドから rm / git rm の削除対象パスを拾う (保守的)。
///
/// `rm` または `git rm` トークンの後ろの、フラグでない (先頭 `-` でない) トークンをパスとする。
/// シェル区切り (`;` `&&` `|`) で止める。クォートは剥がす。完全な shell 解析はしない (近似)。
fn rm_targets(command: &str) -> Vec<String> {
    let tokens: Vec<&str> = command.split_whitespace().collect();
    let mut targets = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        let is_rm = tokens[i] == "rm" || (tokens[i] == "git" && tokens.get(i + 1) == Some(&"rm"));
        if !is_rm {
            i += 1;
            continue;
        }
        let mut j = if tokens[i] == "git" { i + 2 } else { i + 1 };
        while j < tokens.len() {
            let t = tokens[j];
            // シェル区切りで止める。
            if matches!(t, ";" | "&&" | "||" | "|" | "&") {
                break;
            }
            // フラグは飛ばす。
            if t.starts_with('-') {
                j += 1;
                continue;
            }
            let cleaned = t.trim_matches(['"', '\'', '`']);
            if !cleaned.is_empty() {
                targets.push(cleaned.to_string());
            }
            j += 1;
        }
        i = j;
    }
    targets
}

/// Bash コマンドからファイルへの書き込み先パスを拾う (保守的)。
///
/// シェルでの編集経路を層ゲートへ載せる。実機 Codex の subagent (worker) は apply_patch を持たず
/// 読み書きを全て exec_command (シェル) で行うため、リダイレクトや sed -i での guarded 契約面書き込みが
/// apply_patch 編集と同じゲートを通らないと subagent 内で機械強制が抜ける
/// (`docs/validation/20260620-Phase9-オーケストレーション実機.md`)。
///
/// 拾うのはリダイレクト (`>` `>>` の連結/分離・`2>` `&>` 等)・`tee`・`sed -i`・`cp`/`mv` の宛先・`dd of=`。
/// 完全な shell 解析はしない (近似)。過剰に拾っても層 glob に当たらなければ無害なので、取りこぼしより
/// 過剰側へ倒す。`rm`/`git rm` の削除は `rm_targets` が別に拾う。
pub fn write_targets(command: &str) -> Vec<String> {
    let tokens: Vec<&str> = command.split_whitespace().collect();
    let mut targets = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        let t = tokens[i];
        if let Some(rest) = redirect_suffix(t) {
            // 連結形 (`>>file`) は接尾がパス。分離形 (`>>` の次) は次トークンがパス。
            if rest.is_empty() {
                if let Some(next) = tokens.get(i + 1) {
                    push_write_target(&mut targets, next);
                    i += 2;
                    continue;
                }
            } else {
                push_write_target(&mut targets, rest);
            }
        } else if t == "tee" {
            // 区切りまでの非フラグトークンが全て書き込み先。
            for x in tokens[i + 1..].iter().take_while(|x| !is_shell_sep(x)) {
                if !x.starts_with('-') {
                    push_write_target(&mut targets, x);
                }
            }
        } else if t == "sed" {
            // -i / --in-place があれば in-place 編集。区切りまでの非フラグを宛先とみなす
            // (script も拾うが層 glob に当たらず無害)。
            let seg: Vec<&str> = tokens[i + 1..]
                .iter()
                .take_while(|x| !is_shell_sep(x))
                .copied()
                .collect();
            let in_place = seg
                .iter()
                .any(|x| x.starts_with("-i") || x.starts_with("--in-place"));
            if in_place {
                for x in seg.iter().filter(|x| !x.starts_with('-')) {
                    push_write_target(&mut targets, x);
                }
            }
        } else if t == "cp" || t == "mv" {
            // 宛先 = 区切り前の最後の非フラグトークン。
            if let Some(dst) = tokens[i + 1..]
                .iter()
                .take_while(|x| !is_shell_sep(x))
                .filter(|x| !x.starts_with('-'))
                .last()
            {
                push_write_target(&mut targets, dst);
            }
        } else if let Some(of) = t.strip_prefix("of=") {
            push_write_target(&mut targets, of);
        }
        i += 1;
    }
    targets
}

/// シェルのコマンド区切りトークンか。
fn is_shell_sep(token: &str) -> bool {
    matches!(token, ";" | "&&" | "||" | "|" | "&")
}

/// クォートを剥がして書き込み先候補へ追加する (空なら捨てる)。
fn push_write_target(targets: &mut Vec<String>, token: &str) {
    let cleaned = token.trim_matches(['"', '\'', '`']);
    if !cleaned.is_empty() {
        targets.push(cleaned.to_string());
    }
}

/// リダイレクト演算子トークンを解析し、書き込み先の接尾を返す。
///
/// `>` `>>` `>|`、fd 付き (`1>` `2>>`)、両出力 (`&>` `&>>`) を書き込みとみなす。接尾が空なら
/// 宛先は次トークン。fd 複製 (`2>&1` のような数字 fd + `&`) はファイル書き込みでないので除く。
/// 読み込み (`<`) は対象外。
fn redirect_suffix(token: &str) -> Option<&str> {
    let bytes = token.as_bytes();
    let mut k = 0;
    let amp_prefix = bytes.first() == Some(&b'&');
    if amp_prefix {
        k += 1;
    } else {
        while bytes.get(k).is_some_and(u8::is_ascii_digit) {
            k += 1;
        }
    }
    if bytes.get(k) != Some(&b'>') {
        return None;
    }
    k += 1;
    if bytes.get(k) == Some(&b'>') {
        k += 1;
    }
    if bytes.get(k) == Some(&b'|') {
        k += 1;
    }
    let rest = &token[k..];
    // fd 数字接頭で接尾が `&...` は fd 複製 (例 `2>&1`)。ファイル書き込みでない。
    if !amp_prefix && rest.starts_with('&') {
        return None;
    }
    Some(rest)
}

/// commit ゲートの検証結果。呼び出し側が検査を実行して組む。
pub enum VerifyOutcome {
    /// 検査が未設定。完了を機械検証できない。
    NoChecks,
    /// 全検査が通過。
    Passed,
    /// 1 つ以上の検査が失敗。失敗した検査名を持つ。
    Failed { failed: Vec<String> },
}

/// git commit 直前の完了ゲート。
///
/// 完了検証は機械強制 (`docs/decisions/20260611-制御方針.md`): 検査失敗は常に deny (phase 不問)。
/// 未承認 gate (open 来歴) と quality 違反は状態適応: 保守 (maintenance) では block、それ以外は警告
/// (quality はコアゲートでないため state 適応。`docs/decisions/20260614-Phase6-quality適応度関数.md`)。
/// 検査未設定も機械検証できないため警告する (block はしない。verify.run の no-checks と整合)。
///
/// `quality_blocking` は層×phase の合成で block と判定済みの品質バー違反 (mcp が `gate::commit_blocks`
/// で組む。guarded 層は phase 不問・他は保守でのみ block)。`quality_advisory` は止めない違反の 1 行サマリ。
/// `decay_blocking` は構造的腐敗 (done未検証・ゾンビ) の 1 行サマリ。保守では block・他は警告。
/// 放置/孤立/重複/来歴鮮度は狼少年化を避け commit を止めず、verify.run と next の助言に留める
/// (`docs/decisions/20260614-Phase7-腐敗検知の中核.md`)。
pub fn commit_gate(
    verify: &VerifyOutcome,
    open_gates: usize,
    quality_blocking: &[String],
    quality_advisory: &[String],
    decay_blocking: &[String],
    phase: Phase,
) -> HookDecision {
    if let VerifyOutcome::Failed { failed } = verify {
        return HookDecision::Deny {
            reason: format!(
                "Verification failed before commit. Failing checks: {}. Fix the code (or the checks) until they pass, then commit again.",
                failed.join(", ")
            ),
        };
    }

    // 層×phase の合成で block と判定された quality 違反を block (品質バーを機械で守る)。
    // guarded 層は phase 不問・他は保守でのみここへ来る (caller が gate::commit_blocks で振り分け済み)。
    if !quality_blocking.is_empty() {
        return HookDecision::Deny {
            reason: format!(
                "{} quality bar violation(s) block this commit: {}. Fix them, or relax the rule in quality.toml. Violations in a guarded layer block regardless of phase.",
                quality_blocking.len(),
                quality_blocking.join("; ")
            ),
        };
    }

    // 保守状態では構造的腐敗 (done未検証・ゾンビ) を block (嘘 done と壊れた依存グラフを断つ)。
    if !decay_blocking.is_empty() && phase == Phase::Maintenance {
        return HookDecision::Deny {
            reason: format!(
                "{} structural decay finding(s) and the project is in maintenance: {}. Resolve them, or relax [decay] in quality.toml, before committing.",
                decay_blocking.len(),
                decay_blocking.join("; ")
            ),
        };
    }

    // 保守状態では未承認 gate を block (回帰防止優先)。
    if open_gates > 0 && phase == Phase::Maintenance {
        return HookDecision::Deny {
            reason: format!(
                "{open_gates} decision(s) are still open and awaiting human judgment, and the project is in maintenance. When the human decides on each, call gate.approve yourself (its CLI confirmation prompt is the human's approval) before committing."
            ),
        };
    }

    let mut notes: Vec<String> = Vec::new();
    if matches!(verify, VerifyOutcome::NoChecks) {
        notes.push(
            "No verification checks are configured, so completion could not be machine-verified. Confirm the work is done, or add [[verify.checks]] to config.toml."
                .to_string(),
        );
    }
    if open_gates > 0 {
        notes.push(format!(
            "{open_gates} decision(s) are still open and awaiting human judgment. When the human decides on each, call gate.approve yourself (its CLI confirmation prompt is the human's approval), or confirm they are intentional before committing."
        ));
    }
    if !quality_advisory.is_empty() {
        notes.push(format!(
            "{} quality bar violation(s): {}. Address them, or relax the rule in quality.toml.",
            quality_advisory.len(),
            quality_advisory.join("; ")
        ));
    }
    if !decay_blocking.is_empty() {
        notes.push(format!(
            "{} structural decay finding(s): {}. Resolve them, or relax [decay] in quality.toml.",
            decay_blocking.len(),
            decay_blocking.join("; ")
        ));
    }

    if notes.is_empty() {
        HookDecision::Allow
    } else {
        HookDecision::Remind {
            message: notes.join(" "),
        }
    }
}

/// コマンドが git commit か (plumbing の git commit-tree は除く)。
pub fn is_git_commit(command: &str) -> bool {
    let re = regex::Regex::new(r"\bgit\s+commit(\s|$)").expect("git commit pattern is valid");
    re.is_match(command)
}

/// Stop (ターン終了) の判断結果。
pub enum StopDecision {
    /// 終了を受理する (出力無し・終了コード 0)。
    Accept,
    /// 終了させず 1 度だけ継続させる。reason が新しい継続プロンプトになる。
    Continue { reason: String },
}

/// Stop 時の決定。
///
/// 完了前に verify・判断記録を促す (誘導)。`stop_hook_active` (既に継続済み) なら
/// 受理してループを避ける。継続は 1 ターンにつき高々 1 回。
///
/// `dirty` は作業ツリーに verify 対象の変更があるか (.owox 配下は除外して呼び出し側が判定)。
/// `verify_rearmed` は前回 checklist を促してから verify.run が走ったか (まだ一度も促していない時も
/// 真。呼び出し側が「前回促した時の verify 署名」と今の verify 署名を比べて判定)。未検証の変更が
/// 続く 1 つのエピソードでは checklist を高々 1 回だけ出し、その後の連続編集では黙る。verify.run が
/// 走ると次の未検証エピソードとして再武装し、また高々 1 回促す。これで編集が進むたび毎ターン催促
/// される過多を断つ (合否強制は commit ゲートが担うので促しは礼儀に留める)。
///
/// `open_gates` は未承認 gate の数。`gates_changed` は前回促した時から未承認 gate の顔ぶれが
/// 変わったか (呼び出し側が open 来歴 ID の署名で判定)。open gate も「変わった時だけ」一度表面化する。
/// 人間待ちのゲートは AI が毎ターン蒸し返すべきものでないので、同じ顔ぶれが続く間は黙る
/// (狼少年化を避ける。保守 phase の commit ゲートが別途 open gate を block するので機械強制は不変)。
/// クリーン かつ 促す変化が無いなら何もすることが無いので素通りする
/// (`docs/handoff/20260613-Phase4対話検証で見つけた粗の改善.md`)。
///
/// stop は毎ターン末に発火するため検査の再実行はしない (重い)。機械強制の完了検証は
/// commit ゲート (`commit_gate`) が担う。stop は変更の確認促しと未解決 gate の表面化に絞る。
///
/// `verified_current` は今の作業ツリーが直前の verify.run と同一内容か (呼び出し側が署名で判定)。
/// true なら既に検証済みで以降変更が無いので verify を促す checklist を出さない。合否は問わない
/// (Stop は走らせたかの誘導で、合否強制は commit ゲートが担う)。これで verify.run 後に未コミットの
/// 変更が残るだけで毎ターン催促されるノイズを避ける。
pub fn stop_decision(
    stop_hook_active: bool,
    open_gates: usize,
    gates_changed: bool,
    dirty: bool,
    verified_current: bool,
    verify_rearmed: bool,
) -> StopDecision {
    if stop_hook_active {
        return StopDecision::Accept;
    }
    // 未検証の変更があり、かつ前回促してから verify.run が走った (= 新しい未検証エピソード) 時だけ
    // checklist を出す。同じ未検証エピソードの連続編集では再び促さず、毎ターンの催促を避ける。
    let want_checklist = dirty && !verified_current && verify_rearmed;
    // 未承認 gate も顔ぶれが変わった時だけ表面化する (毎ターンの蒸し返しを避ける)。
    let want_gates = open_gates > 0 && gates_changed;
    // 促す変化が無い (新規変更なし・gate の顔ぶれも不変) なら黙って終わる。
    if !want_checklist && !want_gates {
        return StopDecision::Accept;
    }

    let mut parts: Vec<String> = Vec::new();
    if want_checklist {
        parts.push(STOP_CHECKLIST.to_string());
    }
    if want_gates {
        parts.push(format!(
            "{open_gates} decision(s) are still open and awaiting human judgment; call the next tool to see them."
        ));
    }
    StopDecision::Continue {
        reason: parts.join(" "),
    }
}

/// 変更があるターンの完了前チェックリスト。継続プロンプトとして AI へ渡す。
/// 一時的・作業状態は decision でなく task.note へ逃がすよう誘導する (来歴の乱立を防ぐ)。
const STOP_CHECKLIST: &str = "Before finishing this turn: run verify.run for the code you changed, and record only durable design or direction decisions with decision.record — use task.note for transient working memos. If you have already done this, you may stop.";

#[cfg(test)]
mod tests {
    use super::*;

    fn is_remind(d: &HookDecision) -> bool {
        matches!(d, HookDecision::Remind { .. })
    }
    fn is_deny(d: &HookDecision) -> bool {
        matches!(d, HookDecision::Deny { .. })
    }
    fn is_allow(d: &HookDecision) -> bool {
        matches!(d, HookDecision::Allow)
    }
    fn decide(tool_name: &str, command: Option<&str>) -> HookDecision {
        pre_tool_use_decision(tool_name, command, &[])
    }

    #[test]
    fn irreversible_denies() {
        // 不可逆は機械強制で deny。commit 判定とは独立。
        assert!(is_deny(&decide("Bash", Some("git push --force"))));
    }

    #[test]
    fn plain_bash_allows() {
        // pre_tool_use_decision は不可逆判定のみ。commit はここでは Allow
        // (完了ゲートは commit_gate が別に判定する)。
        assert!(is_allow(&decide("Bash", Some("git status"))));
        assert!(is_allow(&decide("Bash", Some("ls"))));
        assert!(is_allow(&decide("Bash", Some("git commit -m 'x'"))));
        // plumbing の git commit-tree は commit 扱いしない。
        assert!(!is_git_commit("git commit-tree abc123"));
        assert!(is_git_commit("git commit -m 'x'"));
        assert!(is_git_commit("git commit"));
    }

    #[test]
    fn non_bash_allows() {
        assert!(is_allow(&decide("apply_patch", None)));
    }

    #[test]
    fn reading_canon_via_bash_is_denied() {
        // 指針 canon を内容読みするコマンドは止める。
        assert!(is_deny(&decide("Bash", Some("cat .owox/glossary.md"))));
        assert!(is_deny(&decide("Bash", Some("grep -r foo .owox"))));
        assert!(is_deny(&decide("Bash", Some("head .owox/rules.md"))));
    }

    #[test]
    fn skills_dir_and_listing_are_allowed() {
        // skills は著作作業域なので読める。
        assert!(is_allow(&decide(
            "Bash",
            Some("cat .owox/skills/tidy/SKILL.md")
        )));
        // 列挙は内容読みでないので止めない。
        assert!(is_allow(&decide("Bash", Some("ls .owox"))));
        // canon に触れない読みは当然許可。
        assert!(is_allow(&decide("Bash", Some("cat README.md"))));
    }

    #[test]
    fn commit_gate_denies_on_failed_verify_any_phase() {
        let outcome = VerifyOutcome::Failed {
            failed: vec!["cargo test".to_string()],
        };
        assert!(is_deny(&commit_gate(
            &outcome,
            0,
            &[],
            &[],
            &[],
            Phase::Initial
        )));
        assert!(is_deny(&commit_gate(
            &outcome,
            0,
            &[],
            &[],
            &[],
            Phase::Maintenance
        )));
    }

    #[test]
    fn commit_gate_allows_when_passed_and_no_open_gates() {
        assert!(is_allow(&commit_gate(
            &VerifyOutcome::Passed,
            0,
            &[],
            &[],
            &[],
            Phase::Initial
        )));
    }

    #[test]
    fn commit_gate_warns_on_no_checks_or_open_gates() {
        assert!(is_remind(&commit_gate(
            &VerifyOutcome::NoChecks,
            0,
            &[],
            &[],
            &[],
            Phase::Initial
        )));
        assert!(is_remind(&commit_gate(
            &VerifyOutcome::Passed,
            2,
            &[],
            &[],
            &[],
            Phase::Initial
        )));
    }

    #[test]
    fn commit_gate_blocks_open_gates_in_maintenance() {
        // 保守では未承認 gate を block。検査通過でも止める。
        assert!(is_deny(&commit_gate(
            &VerifyOutcome::Passed,
            1,
            &[],
            &[],
            &[],
            Phase::Maintenance
        )));
        // 保守でも open gate が無ければ通す。
        assert!(is_allow(&commit_gate(
            &VerifyOutcome::Passed,
            0,
            &[],
            &[],
            &[],
            Phase::Maintenance
        )));
    }

    #[test]
    fn commit_gate_quality_blocking_denies_advisory_reminds() {
        // 層×phase の振り分けは caller (gate::commit_blocks) が担い、commit_gate は
        // blocking → deny・advisory → remind を組むだけ (`gate.rs` に検算)。
        let v = vec!["src/big.rs [budget]: 500 lines exceed the 400-line budget".to_string()];
        // blocking なら phase 不問で deny。
        assert!(is_deny(&commit_gate(
            &VerifyOutcome::Passed,
            0,
            &v,
            &[],
            &[],
            Phase::Initial
        )));
        // advisory のみなら remind。
        assert!(is_remind(&commit_gate(
            &VerifyOutcome::Passed,
            0,
            &[],
            &v,
            &[],
            Phase::Initial
        )));
        // 違反なしなら通す。
        assert!(is_allow(&commit_gate(
            &VerifyOutcome::Passed,
            0,
            &[],
            &[],
            &[],
            Phase::Initial
        )));
    }

    #[test]
    fn commit_gate_structural_decay_is_phase_adaptive() {
        let d = vec![
            "20260101-x [unverified-done]: marked done but has no verification link".to_string(),
        ];
        // 保守では構造的腐敗を block。
        assert!(is_deny(&commit_gate(
            &VerifyOutcome::Passed,
            0,
            &[],
            &[],
            &d,
            Phase::Maintenance
        )));
        // 初期では警告に留める (block しない)。
        assert!(is_remind(&commit_gate(
            &VerifyOutcome::Passed,
            0,
            &[],
            &[],
            &d,
            Phase::Initial
        )));
    }

    fn is_layer_allow(g: &LayerGate) -> bool {
        matches!(g, LayerGate::Allow)
    }
    fn is_layer_deny(g: &LayerGate) -> bool {
        matches!(g, LayerGate::Deny { .. })
    }
    fn no_auth() -> Vec<GateAuthorization> {
        Vec::new()
    }

    #[test]
    fn layer_gate_off_when_not_layered() {
        let q = quality_with_guarded();
        // architecture=flat 相当 (architecture_layered=false) なら層ゲートは効かない。
        assert!(is_layer_allow(&layer_pre_action_gate(
            "Bash",
            Some("rm crates/core/src/lib.rs"),
            None,
            &q,
            false,
            Path::new("."),
            &no_auth(),
        )));
    }

    #[test]
    fn layer_gate_denies_guarded_deletion() {
        let q = quality_with_guarded();
        assert!(is_layer_deny(&layer_pre_action_gate(
            "Bash",
            Some("rm crates/core/src/lib.rs"),
            None,
            &q,
            true,
            Path::new("."),
            &no_auth(),
        )));
        // free 層の削除は通す。
        assert!(is_layer_allow(&layer_pre_action_gate(
            "Bash",
            Some("rm crates/ui/app.rs"),
            None,
            &q,
            true,
            Path::new("."),
            &no_auth(),
        )));
    }

    #[test]
    fn layer_gate_denies_contract_surface_edit() {
        let q = quality_with_guarded();
        assert!(is_layer_deny(&layer_pre_action_gate(
            "Edit",
            None,
            Some("crates/core/src/ports/db.rs"),
            &q,
            true,
            Path::new("."),
            &no_auth(),
        )));
        // guarded 層でも契約面でない編集は通す (操作前は止めない・他経路で助言)。
        assert!(is_layer_allow(&layer_pre_action_gate(
            "Edit",
            None,
            Some("crates/core/src/impl.rs"),
            &q,
            true,
            Path::new("."),
            &no_auth(),
        )));
    }

    /// apply_patch の削除・契約面編集を実機経路 (パッチ本文 + 絶対パス) で止める。
    /// Codex は削除も編集も apply_patch で送り、パッチ本文を command に置き、パスは絶対。
    #[test]
    fn layer_gate_handles_apply_patch_with_absolute_paths() {
        let q = quality_with_guarded();
        let wd = Path::new("/repo");
        // guarded 層のファイル削除 (絶対パス) を止める。
        let del = "*** Begin Patch\n*** Delete File: /repo/crates/core/src/lib.rs\n*** End Patch\n";
        assert!(is_layer_deny(&layer_pre_action_gate(
            "apply_patch",
            Some(del),
            None,
            &q,
            true,
            wd,
            &no_auth(),
        )));
        // guarded 層の契約面編集 (Update File・絶対パス) を止める。
        let edit = "*** Begin Patch\n*** Update File: /repo/crates/core/src/ports/db.rs\n@@\n-a\n+b\n*** End Patch\n";
        assert!(is_layer_deny(&layer_pre_action_gate(
            "apply_patch",
            Some(edit),
            None,
            &q,
            true,
            wd,
            &no_auth(),
        )));
        // free 層の削除・guarded 非契約面の更新は通す。
        let ok = "*** Begin Patch\n*** Delete File: /repo/crates/ui/app.rs\n*** Update File: /repo/crates/core/src/impl.rs\n@@\n-a\n+b\n*** End Patch\n";
        assert!(is_layer_allow(&layer_pre_action_gate(
            "apply_patch",
            Some(ok),
            None,
            &q,
            true,
            wd,
            &no_auth(),
        )));
    }

    /// シェル書き込み経路: 実機 Codex の subagent は apply_patch を持たず編集をシェルで行う。
    /// リダイレクト等で guarded 契約面へ書くと apply_patch 編集と同じく止める (機械強制の subagent 成立)。
    #[test]
    fn layer_gate_denies_shell_write_to_contract_surface() {
        let q = quality_with_guarded();
        let wd = Path::new("/repo");
        // 実機 subagent が使った形 (`sh -c 'printf ... >> <契約面>'`)。相対パス・末尾クォート込み。
        let append = "sh -c 'printf \"x\\n\" >> crates/core/src/ports/db.rs'";
        assert!(is_layer_deny(&layer_pre_action_gate(
            "Bash",
            Some(append),
            None,
            &q,
            true,
            wd,
            &no_auth(),
        )));
        // 分離形リダイレクト・絶対パスでも止める。
        let truncate = "printf x > /repo/crates/core/src/ports/db.rs";
        assert!(is_layer_deny(&layer_pre_action_gate(
            "Bash",
            Some(truncate),
            None,
            &q,
            true,
            wd,
            &no_auth(),
        )));
        // tee / sed -i / cp の宛先も契約面なら止める。
        for cmd in [
            "echo x | tee crates/core/src/ports/db.rs",
            "sed -i 's/a/b/' crates/core/src/ports/db.rs",
            "cp /tmp/x crates/core/src/ports/db.rs",
        ] {
            assert!(
                is_layer_deny(&layer_pre_action_gate(
                    "Bash",
                    Some(cmd),
                    None,
                    &q,
                    true,
                    wd,
                    &no_auth(),
                )),
                "should deny: {cmd}"
            );
        }
        // guarded 非契約面・free 層への書き込み、fd 複製 (`2>&1`) は通す (素通り維持)。
        for cmd in [
            "printf x >> crates/core/src/impl.rs",
            "printf x >> crates/ui/app.rs",
            "run_thing > /dev/null 2>&1",
        ] {
            assert!(
                is_layer_allow(&layer_pre_action_gate(
                    "Bash",
                    Some(cmd),
                    None,
                    &q,
                    true,
                    wd,
                    &no_auth(),
                )),
                "should allow: {cmd}"
            );
        }
    }

    /// write_targets の抽出: 連結/分離リダイレクト・tee・sed -i・cp/mv・dd of= を拾い、
    /// fd 複製 (`2>&1`) と読み込み (`<`) は拾わない。
    #[test]
    fn write_targets_extracts_destinations() {
        assert_eq!(write_targets("printf x >> a.txt"), vec!["a.txt"]);
        assert_eq!(write_targets("printf x >b.txt"), vec!["b.txt"]);
        assert_eq!(
            write_targets("echo x | tee c.txt d.txt"),
            vec!["c.txt", "d.txt"]
        );
        // sed -i は script も宛先候補に拾う (過剰側へ倒す設計・層 glob に当たらず無害)。
        assert!(write_targets("sed -i 's/a/b/' e.txt").contains(&"e.txt".to_string()));
        assert_eq!(write_targets("cp src.txt f.txt"), vec!["f.txt"]);
        assert_eq!(write_targets("mv -f src.txt g.txt"), vec!["g.txt"]);
        assert_eq!(write_targets("dd if=/dev/zero of=h.img"), vec!["h.img"]);
        // 書き込みでない経路は空。
        assert!(write_targets("run > /dev/null 2>&1").contains(&"/dev/null".to_string()));
        assert!(!write_targets("run 2>&1").iter().any(|t| t.contains('&')));
        assert!(write_targets("cat < in.txt").is_empty());
        assert!(write_targets("grep x file.txt").is_empty());
    }

    /// 承認で解凍: guarded 対象が承認済みゲートに覆われれば Thaw、覆われなければ Deny。
    #[test]
    fn layer_gate_thaws_when_authorized() {
        let q = quality_with_guarded();
        let wd = Path::new("/repo");
        // engine 削除 + ports 契約面編集 (両方 guarded) を 1 つの承認ゲートが覆う。
        let patch = "*** Begin Patch\n*** Delete File: /repo/crates/core/src/lib.rs\n*** Update File: /repo/crates/core/src/ports/db.rs\n@@\n-a\n+b\n*** End Patch\n";
        let auth = vec![GateAuthorization {
            id: "20260619-g".to_string(),
            paths: vec![
                "crates/core/src/lib.rs".to_string(),
                "crates/core/src/ports/db.rs".to_string(),
            ],
        }];
        match layer_pre_action_gate("apply_patch", Some(patch), None, &q, true, wd, &auth) {
            LayerGate::Thaw { decision_ids } => assert_eq!(decision_ids, vec!["20260619-g"]),
            _ => panic!("should thaw when all guarded targets are authorized"),
        }
        // 片方しか覆わなければ deny (未承認の対象が残る)。
        let partial = vec![GateAuthorization {
            id: "20260619-g".to_string(),
            paths: vec!["crates/core/src/lib.rs".to_string()],
        }];
        assert!(is_layer_deny(&layer_pre_action_gate(
            "apply_patch",
            Some(patch),
            None,
            &q,
            true,
            wd,
            &partial,
        )));
    }

    /// 未承認の guarded 対象が複数ある時、deny は全てを 1 メッセージで名指しする (承認1回で済むよう導く)。
    #[test]
    fn layer_gate_deny_lists_all_uncovered_targets() {
        let q = quality_with_guarded();
        let wd = Path::new("/repo");
        let patch = "*** Begin Patch\n*** Delete File: /repo/crates/core/src/lib.rs\n*** Update File: /repo/crates/core/src/ports/db.rs\n@@\n-a\n+b\n*** End Patch\n";
        match layer_pre_action_gate("apply_patch", Some(patch), None, &q, true, wd, &no_auth()) {
            LayerGate::Deny { reason } => {
                assert!(reason.contains("crates/core/src/lib.rs"), "{reason}");
                assert!(reason.contains("crates/core/src/ports/db.rs"), "{reason}");
                assert!(reason.contains("authorizes"), "{reason}");
            }
            _ => panic!("should deny and name every uncovered guarded target"),
        }
    }

    /// パッチパーサ: Add/Update/Delete/Move to を拾い、差分本文は無視する。
    #[test]
    fn parse_patch_changes_extracts_ops() {
        let patch = "*** Begin Patch\n*** Add File: a.rs\n+x\n*** Update File: b.rs\n@@\n-y\n+z\n*** Delete File: c.rs\n*** Move to: d.rs\n*** End Patch\n";
        let changes = parse_patch_changes(patch);
        assert_eq!(
            changes,
            vec![
                PatchChange {
                    op: PatchOp::Add,
                    path: "a.rs".to_string()
                },
                PatchChange {
                    op: PatchOp::Update,
                    path: "b.rs".to_string()
                },
                PatchChange {
                    op: PatchOp::Delete,
                    path: "c.rs".to_string()
                },
                PatchChange {
                    op: PatchOp::Add,
                    path: "d.rs".to_string()
                },
            ]
        );
    }

    fn quality_with_guarded() -> Quality {
        let toml = "[[layers]]\npaths = [\"crates/core/src/**\"]\nautonomy = \"guarded\"\ncontract_surface = [\"crates/core/src/ports/**\"]\n\n[[layers]]\npaths = [\"crates/ui/**\"]\nautonomy = \"free\"\n";
        Quality::from_toml(toml).unwrap()
    }

    #[test]
    fn stop_continues_once_then_accepts() {
        // 未検証の変更があり再武装済みなら 1 度継続、既継続なら受理。
        assert!(matches!(
            stop_decision(false, 0, false, true, false, true),
            StopDecision::Continue { .. }
        ));
        assert!(matches!(
            stop_decision(true, 0, false, true, false, true),
            StopDecision::Accept
        ));
    }

    #[test]
    fn stop_accepts_when_dirty_but_not_rearmed() {
        // dirty でも前回促してから verify.run が走っていなければ (再武装なし) 黙る。
        // 同じ未検証エピソードの連続編集で毎ターン催促しない。
        assert!(matches!(
            stop_decision(false, 0, false, true, false, false),
            StopDecision::Accept
        ));
    }

    #[test]
    fn stop_continues_again_after_verify_rearms() {
        // verify.run が走り再武装されたら、次の未検証編集でまた高々 1 回促す。
        assert!(matches!(
            stop_decision(false, 0, false, true, false, true),
            StopDecision::Continue { .. }
        ));
    }

    #[test]
    fn stop_accepts_when_verified_current_even_if_rearmed() {
        // 直前に verify.run を走らせた内容のままなら checklist を出さない
        // (verify.run 済み・以降変更なし。合否は問わない)。
        assert!(matches!(
            stop_decision(false, 0, false, true, true, true),
            StopDecision::Accept
        ));
    }

    #[test]
    fn stop_surfaces_gates_even_when_verified_current() {
        // verify 済みで checklist を抑えても、未決 gate の顔ぶれが変われば別途表面化する。
        assert!(matches!(
            stop_decision(false, 2, true, true, true, true),
            StopDecision::Continue { .. }
        ));
    }

    #[test]
    fn stop_accepts_when_clean_and_no_open_gates() {
        // 変更なし・未決なしなら黙って終わる (ノイズを出さない)。
        assert!(matches!(
            stop_decision(false, 0, false, false, false, false),
            StopDecision::Accept
        ));
    }

    #[test]
    fn stop_mentions_open_gates_when_changed() {
        // クリーンでも未決 gate の顔ぶれが変わった時は一度表面化する。
        if let StopDecision::Continue { reason } =
            stop_decision(false, 3, true, false, false, false)
        {
            assert!(reason.contains('3'));
        } else {
            panic!("should continue");
        }
    }

    #[test]
    fn stop_accepts_when_open_gates_unchanged() {
        // 同じ未決 gate が続く間は黙る (人間待ちを毎ターン蒸し返さない)。
        assert!(matches!(
            stop_decision(false, 3, false, false, false, false),
            StopDecision::Accept
        ));
    }

    fn canon_with_glossary(entries: &[(&str, &str)]) -> Canon {
        use crate::model::{
            Brand, Canon, Context, Glossary, GlossaryEntry, Practices, Rules, Settings, State,
            Targets, VerifyConfig,
        };
        Canon {
            brand: Brand {
                vision: "v".to_string(),
                ..Brand::default()
            },
            rules: Rules::default(),
            context: Context::default(),
            glossary: Glossary {
                entries: entries
                    .iter()
                    .map(|(t, d)| GlossaryEntry {
                        term: t.to_string(),
                        aliases: Vec::new(),
                        definition: d.to_string(),
                    })
                    .collect(),
                ..Glossary::default()
            },
            practices: Practices::default(),
            targets: Targets::default(),
            verify: VerifyConfig::default(),
            quality: crate::quality::Quality::default(),
            state: State::default(),
            settings: Settings::default(),
            profile: crate::profile::Profile::default(),
            agents: crate::agents::Agents::default(),
            release: crate::release::Release::default(),
        }
    }

    fn none_set() -> std::collections::HashSet<String> {
        std::collections::HashSet::new()
    }

    /// 状態適応の回帰防止: state.toml が maintenance の時、session 開始の文脈が
    /// maintenance の振る舞い案内を含む (再起動後も phase が効くことを固定する)。
    #[test]
    fn session_start_injects_maintenance_guidance() {
        let mut canon = canon_with_glossary(&[]);
        canon.state = crate::model::State {
            phase: crate::model::Phase::Maintenance,
        };
        let out = floor_context(&canon);
        assert!(out.contains("Phase: maintenance."));
        assert!(out.contains("maintenance phase"));
        assert!(out.contains("regression test"));
    }

    /// glossary / practices は床へ一覧しない。lookup 導線だけ残す。
    #[test]
    fn floor_omits_glossary_and_practices() {
        let mut canon = canon_with_glossary(&[("a", "da"), ("b", "db"), ("c", "dc")]);
        canon.practices = crate::model::Practices {
            entries: vec![
                crate::model::Practice {
                    date: "20260601".into(),
                    text: "oldest practice".into(),
                },
                crate::model::Practice {
                    date: "20260620".into(),
                    text: "newest practice".into(),
                },
            ],
            rule_entries: vec![],
        };
        let out = floor_context(&canon);
        assert!(!out.contains("## Glossary terms"));
        assert!(!out.contains("\n- a\n"));
        assert!(!out.contains("## Practices"));
        assert!(!out.contains("newest practice"));
        assert!(out.contains("glossary.lookup"));
        assert!(out.contains("practice.lookup"));
    }

    /// 用語が少なくても床へ一覧しない。
    #[test]
    fn floor_never_lists_glossary_terms() {
        let out = floor_context(&canon_with_glossary(&[("alpha", "d")]));
        assert!(!out.contains("\n- alpha\n"));
    }

    /// 床は canon 禁止と entry map だけを残す。
    #[test]
    fn floor_carries_canon_rule_and_entry_map() {
        let out = floor_context(&canon_with_glossary(&[]));
        assert!(out.contains("Do not read or edit the canon"));
        assert!(out.contains("## Entry map"));
        assert!(out.contains("Use kickoff to orient"));
        assert!(out.contains("Use next to choose work"));
        assert!(out.contains("Use context to find what to read"));
        assert!(out.contains("Use verify before finishing"));
        assert!(out.contains("Use review to inspect changes"));
        assert!(out.contains("Use skill to grow or manage skills"));
        assert!(out.contains("rules.lookup, glossary.lookup, and practice.lookup"));
    }

    /// 床は Vision を残し、それ以外の長い常設一覧を落とす。
    #[test]
    fn floor_omits_rules_brand_lists_and_style() {
        let mut canon = canon_with_glossary(&[]);
        canon.brand.style = vec!["short sentences".to_string()];
        canon.brand.values = vec!["clarity".to_string()];
        canon.rules.change_policy = vec!["match existing style".to_string()];
        let out = floor_context(&canon);
        assert!(out.contains("## Vision"));
        assert!(!out.contains("## Rules"));
        assert!(!out.contains("match existing style"));
        assert!(!out.contains("## Values"));
        assert!(!out.contains("clarity"));
        assert!(!out.contains("## Style"));
        assert!(!out.contains("short sentences"));
    }

    /// practices は lookup 導線だけを残し、本文を床へ出さない。
    #[test]
    fn floor_points_to_practice_lookup_without_listing_practices() {
        let mut canon = canon_with_glossary(&[]);
        canon.practices = crate::model::Practices {
            entries: vec![crate::model::Practice {
                date: "20260614".to_string(),
                text: "always add a regression test".to_string(),
            }],
            rule_entries: vec![],
        };
        let out = floor_context(&canon);
        assert!(out.contains("practice.lookup"));
        assert!(!out.contains("## Practices"));
        assert!(!out.contains("always add a regression test"));
    }

    /// テスト用: entries に 1 つのエントリを追加するヘルパ。
    fn push_rule_entry(rules: &mut crate::model::Rules, section: &str, text: &str) {
        rules.entries.push(crate::model::RuleEntry {
            phase: None,
            section: section.to_string(),
            triggers: Vec::new(),
            operations: Vec::new(),
            paths: Vec::new(),
            text: text.to_string(),
        });
    }

    /// policy_injection: rules 語が出たら `## Rules` 本文を push し、出ていなければ素通り。
    #[test]
    fn policy_injects_rules_on_trigger_word() {
        let mut canon = canon_with_glossary(&[]);
        push_rule_entry(&mut canon.rules, "Deletion policy", "keep history");
        // 「delete」で rules を push。
        let inj = policy_injection(
            &canon,
            Phase::Initial,
            "can I delete this file?",
            &none_set(),
            false,
        )
        .unwrap();
        assert!(inj.context.contains("## Rules"));
        assert!(inj.context.contains("keep history"));
        assert_eq!(inj.keys, vec!["policy:rules".to_string()]);
        // 無関係なプロンプトでは出さない。
        assert!(
            policy_injection(&canon, Phase::Initial, "run the tests", &none_set(), false).is_none()
        );
    }

    /// policy_injection: force_rules=true なら語に関わらず rules を届ける (編集直前の経路)。
    #[test]
    fn policy_force_rules_injects_without_word() {
        let mut canon = canon_with_glossary(&[]);
        push_rule_entry(&mut canon.rules, "Safety", "no secrets in logs");
        let inj = policy_injection(&canon, Phase::Initial, "", &none_set(), true).unwrap();
        assert!(inj.context.contains("no secrets in logs"));
    }

    /// policy_injection: 既に注入済みの区分は再注入しない (session 重複排除)。
    #[test]
    fn policy_skips_already_injected() {
        let mut canon = canon_with_glossary(&[]);
        push_rule_entry(&mut canon.rules, "Change policy", "small diffs");
        let already: std::collections::HashSet<String> =
            ["policy:rules".to_string()].into_iter().collect();
        assert!(
            policy_injection(&canon, Phase::Initial, "change the rule", &already, true).is_none()
        );
    }

    /// policy_injection: brand 語で brand リストを push。Vision・全体スタイルは含めない。
    #[test]
    fn policy_injects_brand_on_trigger_word() {
        let mut canon = canon_with_glossary(&[]);
        canon.brand.values = vec!["honesty".to_string()];
        canon.brand.style = vec!["short sentences".to_string()];
        let inj = policy_injection(
            &canon,
            Phase::Initial,
            "what are the project values?",
            &none_set(),
            false,
        )
        .unwrap();
        assert!(inj.context.contains("## Brand"));
        assert!(inj.context.contains("honesty"));
        // 全体スタイルは床に常時なので brand ブロックへ重複させない。
        assert!(!inj.context.contains("short sentences"));
        assert_eq!(inj.keys, vec!["policy:brand".to_string()]);
    }

    /// render_rules_block: 空 rules でも見出しを返し、無い旨を述べる。detect: は出さない。
    #[test]
    fn rules_block_is_labelled_and_hides_detect() {
        let mut rules = crate::model::Rules::default();
        assert!(render_rules_block(&rules).contains("No rules are defined"));
        rules.irreversible = vec![crate::model::Irreversible {
            operation: "git push --force".to_string(),
            reason: "rewrites history".to_string(),
            detect: Some(r"\bgit\s+push".to_string()),
        }];
        let block = render_rules_block(&rules);
        assert!(block.starts_with("## Rules"));
        assert!(block.contains("### Irreversible operations"));
        assert!(block.contains("git push --force"));
        assert!(!block.contains("detect:"));
        assert!(!block.contains("\\bgit"));
    }

    #[test]
    fn glossary_injects_only_present_terms() {
        let canon = canon_with_glossary(&[
            ("target harness", "files owox-harness generates"),
            ("canon", "source of truth under .owox/"),
        ]);
        let out = glossary_injection(&canon, "target harness を再生成して", &none_set()).unwrap();
        assert!(
            out.context
                .contains("target harness: files owox-harness generates")
        );
        // プロンプトに出ていない語は注入しない (最小コンテキスト)。
        assert!(!out.context.contains("source of truth"));
        assert_eq!(out.terms, vec!["target harness".to_string()]);
    }

    #[test]
    fn glossary_match_is_case_insensitive() {
        let canon = canon_with_glossary(&[("Canon", "source of truth")]);
        assert!(glossary_injection(&canon, "explain CANON please", &none_set()).is_some());
    }

    #[test]
    fn glossary_none_when_no_term_present() {
        let canon = canon_with_glossary(&[("target harness", "x")]);
        assert!(glossary_injection(&canon, "just run the tests", &none_set()).is_none());
    }

    #[test]
    fn glossary_skips_already_injected_terms() {
        let canon = canon_with_glossary(&[("canon", "source of truth")]);
        let already: std::collections::HashSet<String> =
            ["canon".to_string()].into_iter().collect();
        // 既に注入済みの語は再注入しない (session 重複排除)。
        assert!(glossary_injection(&canon, "explain canon please", &already).is_none());
    }

    #[test]
    fn session_context_points_to_tools_not_resources() {
        // 読みは tool 一本化。owox:// resource でなく context/next tool へ誘導する。
        let out = floor_context(&canon_with_glossary(&[]));
        assert!(out.contains("Use context to find what to read"));
        assert!(out.contains("Use next to choose work"));
        assert!(!out.contains("owox://"));
    }

    #[test]
    fn session_context_injects_response_language_when_set() {
        let mut canon = canon_with_glossary(&[]);
        canon.settings.language = Some("Japanese".to_string());
        let out = floor_context(&canon);
        assert!(out.contains("Respond to the human in Japanese."));
    }

    #[test]
    fn session_context_omits_language_when_unset() {
        // 未設定なら応答言語を注入しない (モデル既定)。
        let out = floor_context(&canon_with_glossary(&[]));
        assert!(!out.contains("Response language"));
    }

    /// オーケストレーション節は床から外す。
    #[test]
    fn floor_omits_orchestration_section() {
        let out = floor_context(&canon_with_glossary(&[]));
        assert!(!out.contains("## Orchestration"));
        assert!(!out.contains("investigate"));
        assert!(!out.contains("adversarial"));
    }

    /// policy_injection は現在 phase のエントリのみ出す。他 phase のエントリは漏れない。
    #[test]
    fn policy_injection_filters_to_current_phase() {
        let mut canon = canon_with_glossary(&[]);
        // Initial phase のエントリ
        canon.rules.entries.push(crate::model::RuleEntry {
            phase: Some(Phase::Initial),
            section: "Initial only".to_string(),
            triggers: Vec::new(),
            operations: Vec::new(),
            paths: Vec::new(),
            text: "initial-phase-rule".to_string(),
        });
        // Stable phase のエントリ
        canon.rules.entries.push(crate::model::RuleEntry {
            phase: Some(Phase::Stable),
            section: "Stable only".to_string(),
            triggers: Vec::new(),
            operations: Vec::new(),
            paths: Vec::new(),
            text: "stable-phase-rule".to_string(),
        });

        // force_rules=true で Initial を要求
        let inj = policy_injection(&canon, Phase::Initial, "", &none_set(), true).unwrap();
        assert!(
            inj.context.contains("initial-phase-rule"),
            "Initial rule は出るはず: {}",
            inj.context
        );
        assert!(
            !inj.context.contains("stable-phase-rule"),
            "Stable rule は漏れないはず: {}",
            inj.context
        );
    }
}
