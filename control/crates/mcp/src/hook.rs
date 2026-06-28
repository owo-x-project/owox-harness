//! hook 補助サブコマンド。Codex hooks.json の command から直接呼ばれる。
//!
//! stdin で hook イベントの JSON を受け、stdout へ Codex hook 出力 JSON を返す。

use std::collections::HashSet;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use serde::{Deserialize, Serialize};

/// hook イベント入力 (Codex 共通フィールドのうち必要分のみ)。
///
/// 入力フィールドはスネークケース (一次情報 `docs/decisions/20260612-Phase3-hook実装.md`)。
#[derive(Debug, Default, Deserialize)]
struct HookInput {
    /// セッションの作業ディレクトリ。正本 `.owox/` の親。
    #[serde(default)]
    cwd: Option<String>,
    /// 呼ばれた tool 名 (PreToolUse)。Bash / apply_patch / MCP tool 名。
    #[serde(default)]
    tool_name: Option<String>,
    /// tool 固有の入力 (PreToolUse)。
    #[serde(default)]
    tool_input: Option<ToolInput>,
    /// このターンが既に Stop で継続済みか (Stop)。ループ防止に使う。
    #[serde(default)]
    stop_hook_active: bool,
    /// ユーザープロンプト本文 (UserPromptSubmit)。用語定義の push に使う。
    #[serde(default)]
    prompt: Option<String>,
    /// セッション識別子。用語注入の session 単位の重複排除キーに使う。
    #[serde(default)]
    session_id: Option<String>,
}

/// tool 固有入力のうち必要分。Bash はコマンド文字列を、Edit/Write は対象パスを持つ。
#[derive(Debug, Default, Deserialize)]
struct ToolInput {
    #[serde(default)]
    command: Option<String>,
    /// 編集対象パス (Edit / Write)。層別自律度の契約面ゲートに使う。
    #[serde(default)]
    file_path: Option<String>,
}

/// additionalContext で文脈・気づきを注入する出力。
///
/// SessionStart の文脈注入と PreToolUse の非ブロック・リマインダで共用する
/// (どちらも hookEventName + additionalContext の形)。
#[derive(Debug, Serialize)]
struct AdditionalContextOutput {
    #[serde(rename = "hookEventName")]
    hook_event_name: &'static str,
    #[serde(rename = "additionalContext")]
    additional_context: String,
}

#[derive(Debug, Serialize)]
struct HookOutput {
    #[serde(rename = "hookSpecificOutput")]
    hook_specific_output: AdditionalContextOutput,
}

/// Stop で終了させず継続させる出力。reason が新しい継続プロンプトになる。
#[derive(Debug, Serialize)]
struct StopContinue {
    decision: &'static str,
    reason: String,
}

/// PreToolUse で deny する時の出力。`permissionDecision=deny` で止める。
#[derive(Debug, Serialize)]
struct PreToolUseDeny {
    #[serde(rename = "hookEventName")]
    hook_event_name: &'static str,
    #[serde(rename = "permissionDecision")]
    permission_decision: &'static str,
    #[serde(rename = "permissionDecisionReason")]
    permission_decision_reason: String,
}

#[derive(Debug, Serialize)]
struct PreToolUseOutput {
    #[serde(rename = "hookSpecificOutput")]
    hook_specific_output: PreToolUseDeny,
}

/// `owox hook <event>` を捌く。
pub fn run(args: &[String]) -> ExitCode {
    match args.first().map(String::as_str) {
        Some("session-start") => session_start(),
        Some("pre-tool-use") => pre_tool_use(),
        Some("user-prompt-submit") => user_prompt_submit(),
        Some("stop") => stop(),
        Some(other) => {
            eprintln!("owox hook: 未知のイベント: {other}");
            ExitCode::from(2)
        }
        None => {
            eprintln!(
                "owox hook: イベント名が必要 (session-start / pre-tool-use / user-prompt-submit / stop)"
            );
            ExitCode::from(2)
        }
    }
}

/// 1 つのコンテキスト断片を区切って追記する (空なら区切りを足さない)。
fn push_block(buf: &mut String, block: &str) {
    if !buf.is_empty() {
        buf.push_str("\n\n");
    }
    buf.push_str(block);
}

/// UserPromptSubmit。プロンプトに現れた用語定義だけを能動 push する。
///
/// 用語名は床コンテキストに常時。意味が要る時にこの hook が定義を push する。
/// rules / practices は語彙推定でなく、実際の操作と path で出す。ここでは扱わない。
/// 一致が無い・正本が読めない時は素通り (作業を妨げない)。session 内の既出は除く。
fn user_prompt_submit() -> ExitCode {
    let input = read_input();
    let prompt = input.prompt.as_deref().unwrap_or_default();
    if prompt.is_empty() {
        return ExitCode::SUCCESS;
    }
    let prompt_hits = owox_core::extract_term_hits(
        &[owox_core::GlossaryScanText {
            path: "user prompt".to_string(),
            text: prompt.to_string(),
        }],
        "user prompt",
    );
    crate::cache::remember_glossary_hits(
        &owox_dir(input.cwd.as_deref()),
        input.session_id.as_deref(),
        &prompt_hits,
    );

    let Some(canon) = load_canon_from(input.cwd.as_deref()) else {
        return ExitCode::SUCCESS;
    };
    let already = read_injected_terms(input.cwd.as_deref(), input.session_id.as_deref());

    let mut context = String::new();
    let mut keys: Vec<String> = Vec::new();
    if let Some(inj) = owox_core::glossary_injection(&canon, prompt, &already) {
        push_block(&mut context, &inj.context);
        keys.extend(inj.terms);
    }

    // 訂正検知 (決定論シグナル)。AI が編集した後で人間が新たに発話し、作業ツリーに変更が残る時、
    // 「訂正なら correction.note で学びを残せ」と一度だけ促す (`docs/decisions/20260619-承認と自動改善ループ.md`)。
    // どちらが編集したかは hook では分からないため強制でなく助言に留める。session 内で連投しない。
    if session_edited(input.cwd.as_deref(), input.session_id.as_deref())
        && !corr_nudged(input.cwd.as_deref(), input.session_id.as_deref())
        && working_tree_dirty(input.cwd.as_deref())
    {
        push_block(
            &mut context,
            "If this message corrects or overrides what you just did, capture the durable lesson with correction.note so owox can draft it as a proposed practice.",
        );
        mark_corr_nudged(input.cwd.as_deref(), input.session_id.as_deref());
    }

    if context.is_empty() {
        return ExitCode::SUCCESS;
    }
    remember_terms(input.cwd.as_deref(), input.session_id.as_deref(), &keys);

    let output = HookOutput {
        hook_specific_output: AdditionalContextOutput {
            hook_event_name: "UserPromptSubmit",
            additional_context: context,
        },
    };
    if let Ok(json) = serde_json::to_string(&output) {
        println!("{json}");
    }
    ExitCode::SUCCESS
}

/// 編集対象の tool。これらの内容 (patch) に用語が出たら定義を push する。
/// Codex は Edit / Write も tool_name=apply_patch で来る (一次情報) が安全側で全部見る。
/// MultiEdit は Claude Code の複数編集 tool。NotebookEdit は notebook_path で入力形が別のため対象外。
fn is_edit_tool(tool_name: &str) -> bool {
    matches!(tool_name, "apply_patch" | "Edit" | "Write" | "MultiEdit")
}

/// 読取前用語走査の対象。
fn is_read_tool(tool_name: &str) -> bool {
    matches!(tool_name, "Read" | "Open")
}

/// PreToolUse。
///
/// 1. 不可逆操作なら deny で止める (機械強制。最優先)
/// 2. それ以外は、git commit の完了確認リマインダと、編集対象に出た用語定義を
///    additionalContext へまとめて添える (誘導・push)。何も無ければ素通り。
fn pre_tool_use() -> ExitCode {
    let input = read_input();
    let tool_name = input.tool_name.as_deref().unwrap_or_default();
    let command = input.tool_input.as_ref().and_then(|t| t.command.as_deref());
    let file_path = input
        .tool_input
        .as_ref()
        .and_then(|t| t.file_path.as_deref());

    // 使用履歴: 読み・編集・シェル操作を安全な分類で 1 行追記する (best-effort)。
    // 入口コマンド (MCP tool) は serve 側の call_tool で記録するのでここでは拾わない (二重計上回避)。
    let usage_dir = owox_dir(input.cwd.as_deref());
    let today = crate::clock::today_utc();
    if tool_name == "Bash" {
        if let Some(command) = command {
            owox_core::usage::record_shell(&usage_dir, &today, command);
        } else {
            owox_core::usage::record(&usage_dir, &today, "Bash");
        }
    } else if is_edit_tool(tool_name) {
        owox_core::usage::record(&usage_dir, &today, "Edit");
    } else if is_read_tool(tool_name) {
        owox_core::usage::record(&usage_dir, &today, "Read");
    }

    // AI がファイルを編集した印を session へ残す。次の人間プロンプトで訂正検知 nudge の土台にする。
    if is_edit_tool(tool_name) {
        mark_session_edited(input.cwd.as_deref(), input.session_id.as_deref());
    }

    // target 固有の不可逆操作 (rules.md の detect:) も照合する。
    // 正本を読めない場合は既定検出器のみで判断する (作業は妨げない)。
    let canon = load_canon_from(input.cwd.as_deref());
    let irreversible = canon
        .as_ref()
        .map(|c| c.rules.irreversible.as_slice())
        .unwrap_or_default();

    // 不可逆操作は最優先で deny (機械強制)。
    if let owox_core::HookDecision::Deny { reason } =
        owox_core::pre_tool_use_decision(tool_name, command, irreversible)
    {
        return deny_pre_tool_use(reason);
    }

    if is_read_tool(tool_name)
        && let Some(path) = file_path
        && reads_canon_path(path)
    {
        return deny_pre_tool_use(
            "Do not read the project canon under .owox/ directly. Its guidance reaches you through the session context and the owox tools; look up a glossary term with glossary.lookup, and to change or remove canon use canon.propose. You may read and write skills under .owox/skills/.".to_string(),
        );
    }

    if edits_rules_file(&input) {
        return deny_pre_tool_use(
            "Do not edit .owox/rules.md directly. Rules and their delivery triggers are fixed canon. Add with canon.add, and change or remove with canon.propose so a human approves it.".to_string(),
        );
    }

    // 層別自律度の操作前ゲート。architecture=layered の時だけ効く。guarded 層の削除・契約面編集を
    // 操作前に人間ゲートへ回す (`docs/decisions/20260618-Phase9-性質軸適応機構.md`)。
    if let Some(canon) = canon.as_ref() {
        let layered = canon
            .profile
            .resolve()
            .map(|a| a.layered_active())
            .unwrap_or(false);
        let work_dir = input
            .cwd
            .as_deref()
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."));
        // 人間が gate.approve 済みの解凍ゲート (adopted・未消費・authorizes 持ち) を集める。
        let authorized = adopted_authorizations(input.cwd.as_deref());
        match owox_core::layer_pre_action_gate(
            tool_name,
            command,
            file_path,
            &canon.quality,
            layered,
            &work_dir,
            &authorized,
        ) {
            owox_core::LayerGate::Deny { reason } => return deny_pre_tool_use(reason),
            // 解凍: 使った承認ゲートを consumed にして 1 回限りにし、操作を通す。
            owox_core::LayerGate::Thaw { decision_ids } => {
                for id in &decision_ids {
                    let _ = owox_core::mark_gate_consumed(&owox_dir(input.cwd.as_deref()), id);
                }
            }
            owox_core::LayerGate::Allow => {}
        }
    }

    // git commit は完了ゲートへ。検査を再実行し (config の検査)、未承認 gate を数えて判定する。
    // verify 失敗は deny (機械強制)、未設定・open gate は警告。検査未設定なら従来どおり軽い案内。
    if let Some(cmd) = command
        && owox_core::is_git_commit(cmd)
    {
        return commit_gate_decision(input.cwd.as_deref(), canon.as_ref());
    }

    let mut context = String::new();
    // 読取前: 対象ファイルの先頭だけ軽く読み、出た用語の定義だけ届ける。
    // 操作前: operation/path に応じた rules / practices を届ける。
    if let Some(canon) = canon.as_ref() {
        let already = read_injected_terms(input.cwd.as_deref(), input.session_id.as_deref());
        let mut terms: Vec<String> = Vec::new();
        let scan_texts = glossary_scan_texts(&input);
        if !scan_texts.is_empty() {
            let hits = owox_core::extract_term_hits(&scan_texts, "read scan");
            crate::cache::remember_glossary_hits(
                &owox_dir(input.cwd.as_deref()),
                input.session_id.as_deref(),
                &hits,
            );
            let content = scan_texts
                .iter()
                .map(|text| text.text.as_str())
                .collect::<Vec<_>>()
                .join("\n");
            if let Some(inj) = owox_core::glossary_injection(canon, &content, &already) {
                push_block(&mut context, &inj.context);
                terms.extend(inj.terms);
            }
        }
        let delivery_paths = pre_tool_use_delivery_paths(&input);
        let delivery_ops = pre_tool_use_delivery_operations(&input, &delivery_paths);
        if let Ok(selection) = owox_core::select_delivery_for_phase(
            &owox_dir(input.cwd.as_deref()),
            owox_core::DeliveryRequest::for_operations(&delivery_ops, &delivery_paths),
            canon.state.phase,
        ) {
            let block = owox_core::render_delivery_block(&selection);
            if !block.is_empty() {
                push_block(&mut context, &block);
            }
        }
        if !terms.is_empty() {
            remember_terms(input.cwd.as_deref(), input.session_id.as_deref(), &terms);
        }
    }

    if context.is_empty() {
        ExitCode::SUCCESS
    } else {
        remind_pre_tool_use(context)
    }
}

/// git commit の完了ゲート。検査結果を得て未承認 gate を数えて core の判定へ渡す。
///
/// 「古い verify 結果で通さない」ため検査は基本フレッシュに実行する (`docs/decisions/20260613-Phase4-tool記録層.md`)。
/// ただし直前 verify.run と作業ツリーが同一 (署名一致) なら、その時の結果を再利用して検査の二重実行を
/// 避ける (同一ツリー = 同一結果)。検査は cwd (target repo ルート) で走る。正本を読めない時は検査・gate を
/// 見ずに素通り (作業を妨げない)。
fn commit_gate_decision(cwd: Option<&str>, canon: Option<&owox_core::Canon>) -> ExitCode {
    let Some(canon) = canon else {
        return ExitCode::SUCCESS;
    };

    let work_dir = cwd.map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
    // 直前 verify.run と作業ツリーが同一なら、その検査結果を再利用する。一致しない・記録が無い時は
    // フレッシュに検査を走らせる。
    let cached = crate::cache::read_verify_record(&owox_dir(cwd));
    let current_sig = crate::files::tree_signature(&work_dir);
    let reuse = match (&cached, &current_sig) {
        (Some(rec), Some(sig)) => (rec.signature == *sig).then_some(rec),
        _ => None,
    };
    let outcome = match reuse {
        // needs_human = 検査未設定 (verify.rs の判定と整合)。
        Some(rec) => match rec.verification.as_str() {
            "passed" => owox_core::VerifyOutcome::Passed,
            "failed" => owox_core::VerifyOutcome::Failed {
                failed: rec.failed.clone(),
            },
            _ => owox_core::VerifyOutcome::NoChecks,
        },
        None => {
            let results = owox_core::run_checks(&work_dir, &canon.verify.checks);
            if canon.verify.checks.is_empty() {
                owox_core::VerifyOutcome::NoChecks
            } else if results.iter().all(|r| r.passed) {
                owox_core::VerifyOutcome::Passed
            } else {
                owox_core::VerifyOutcome::Failed {
                    failed: results
                        .iter()
                        .filter(|r| !r.passed)
                        .map(|r| r.name.clone())
                        .collect(),
                }
            }
        }
    };

    let open_gates = count_open_gates(cwd);

    // 品質バーとブランド (禁止語) の違反を集める (ファイル列挙は git ls-files)。
    // ブランド違反は kind="brand" で quality と同じく扱う (commit ゲートは kind を問わない)。
    let files = crate::files::list_repo_files(&work_dir);
    let mut quality = owox_core::run_quality(&canon.quality, &work_dir, &files);
    quality.extend(owox_core::run_brand(
        &canon.glossary.forbidden,
        &work_dir,
        &files,
    ));

    // 層×phase の合成で block/助言を振り分ける (`docs/decisions/20260618-Phase9-性質軸適応機構.md`)。
    // architecture=layered の時だけ層別自律度を見る。flat / 層無しは全て Free 扱いで phase 既存挙動
    // (保守のみ block) に一致する。guarded 層の違反は phase 不問で block。
    let layered = canon
        .profile
        .resolve()
        .map(|a| a.layered_active())
        .unwrap_or(false);
    let phase = canon.state.phase;
    let mut quality_blocking: Vec<String> = Vec::new();
    let mut quality_advisory: Vec<String> = Vec::new();
    for v in &quality {
        let autonomy = if layered {
            canon.quality.layer_autonomy(&v.path)
        } else {
            owox_core::Autonomy::Free
        };
        if owox_core::commit_blocks(autonomy, phase) {
            quality_blocking.push(v.summary());
        } else {
            quality_advisory.push(v.summary());
        }
    }

    // 腐敗検知。構造的腐敗 (done未検証・ゾンビ) だけを commit ゲートへ渡す (保守で block)。
    // 放置/孤立/重複/来歴鮮度は狼少年化を避け commit を止めない (next と verify.run の助言に留める)。
    let owox_dir = owox_dir(cwd);
    let tasks = owox_core::list_tasks(&owox_dir).unwrap_or_default();
    let decisions = owox_core::list_decisions(&owox_dir).unwrap_or_default();
    let decay_blocking: Vec<String> = owox_core::run_decay(
        &tasks,
        &decisions,
        &canon.quality.decay,
        &crate::clock::today_utc(),
    )
    .iter()
    .filter(|f| f.is_structural())
    .map(|f| f.summary())
    .collect();

    match owox_core::commit_gate(
        &outcome,
        open_gates,
        &quality_blocking,
        &quality_advisory,
        &decay_blocking,
        canon.state.phase,
    ) {
        owox_core::HookDecision::Deny { reason } => deny_pre_tool_use(reason),
        owox_core::HookDecision::Remind { message } => remind_pre_tool_use(message),
        owox_core::HookDecision::Allow => ExitCode::SUCCESS,
    }
}

/// 未承認 gate (status=open の来歴) の数。読めなければ 0 (作業を妨げない)。
fn count_open_gates(cwd: Option<&str>) -> usize {
    open_gate_ids(cwd).len()
}

/// 未承認 gate (status=open の来歴) の ID を整列して返す。読めなければ空 (作業を妨げない)。
/// Stop の「顔ぶれが変わった時だけ催促」の署名に使う。
fn open_gate_ids(cwd: Option<&str>) -> Vec<String> {
    let mut ids: Vec<String> = owox_core::list_decisions(&owox_dir(cwd))
        .map(|ds| {
            ds.iter()
                .filter(|d| d.status == owox_core::DecisionStatus::Open)
                .map(|d| d.id.clone())
                .collect()
        })
        .unwrap_or_default();
    ids.sort();
    ids
}

/// 人間が gate.approve 済みの解凍ゲート (adopted・未消費・authorizes 持ち) を集める。
/// 層の操作前ゲートへ渡し、guarded 操作の解凍判定に使う。読めなければ空 (= 解凍無し・従来どおり deny)。
fn adopted_authorizations(cwd: Option<&str>) -> Vec<owox_core::GateAuthorization> {
    owox_core::list_decisions(&owox_dir(cwd))
        .map(|ds| {
            ds.into_iter()
                .filter(|d| {
                    d.status == owox_core::DecisionStatus::Adopted
                        && !d.consumed
                        && !d.authorizes.is_empty()
                })
                .map(|d| owox_core::GateAuthorization {
                    id: d.id,
                    paths: d.authorizes,
                })
                .collect()
        })
        .unwrap_or_default()
}

/// PreToolUse の deny 出力。`permissionDecision=deny` で止める。
fn deny_pre_tool_use(reason: String) -> ExitCode {
    let output = PreToolUseOutput {
        hook_specific_output: PreToolUseDeny {
            hook_event_name: "PreToolUse",
            permission_decision: "deny",
            permission_decision_reason: reason.clone(),
        },
    };

    match serde_json::to_string(&output) {
        Ok(json) => {
            println!("{json}");
            ExitCode::SUCCESS
        }
        // JSON を出せなくても止める意図は守る。代替契約 (終了コード 2 + stderr)。
        Err(err) => {
            eprintln!("{reason} (owox: deny 出力生成に失敗: {err})");
            ExitCode::from(2)
        }
    }
}

/// PreToolUse の非ブロック・リマインダ。allow + additionalContext で気づきを添える。
/// 出力に失敗しても作業は妨げない (素通り)。
fn remind_pre_tool_use(message: String) -> ExitCode {
    let output = HookOutput {
        hook_specific_output: AdditionalContextOutput {
            hook_event_name: "PreToolUse",
            additional_context: message,
        },
    };
    if let Ok(json) = serde_json::to_string(&output) {
        println!("{json}");
    }
    ExitCode::SUCCESS
}

/// Stop。完了前に verify・判断記録を促す。継続は 1 ターンに高々 1 回。
/// 作業ツリーがクリーン かつ 未決 gate ゼロ なら黙って終わる (ノイズを出さない)。
fn stop() -> ExitCode {
    let input = read_input();
    let open_ids = open_gate_ids(input.cwd.as_deref());
    let open_gates = open_ids.len();
    let dirty = working_tree_dirty(input.cwd.as_deref());
    let work_dir = input.cwd.as_deref().unwrap_or(".");
    // 作業ツリーの署名。直前 verify.run の署名と突き合わせ「検証済みで以降変更なし」を見る。
    let signature = if dirty {
        crate::files::tree_signature(Path::new(work_dir))
    } else {
        None
    };
    // 直前に verify.run を走らせた時の作業ツリー署名 (無ければ None)。
    let verify_sig = crate::cache::read_verify_signature(&owox_dir(input.cwd.as_deref()));

    // 今が直前 verify.run の内容と同一か。一致なら verify を促す checklist を出さない (合否は問わない。
    // Stop は走らせたかの誘導で、合否強制は commit ゲートが担う)。署名を取れない時は未検証扱いへ倒す。
    let verified_current = match (&signature, &verify_sig) {
        (Some(sig), Some(v)) => sig == v,
        _ => false,
    };

    // 再武装判定。前回 checklist を促した時に覚えた verify 署名と今の verify 署名を比べ、変われば
    // (= 前回促してから verify.run が走った) 再武装する。未促しなら覚えが無く再武装扱い。これで
    // 1 つの未検証エピソードでは高々 1 回だけ促し、編集が進むだけの毎ターンの催促を断つ。
    let marked = read_stop_verify_marker(input.cwd.as_deref(), input.session_id.as_deref());
    let verify_sig_str = verify_sig.as_deref().unwrap_or("");
    let verify_rearmed = match &marked {
        None => true,
        Some(prev) => prev != verify_sig_str,
    };
    // checklist を促す条件 (core と同じ)。促した時だけ今の verify 署名を覚えて再武装を閉じる。
    let want_checklist = dirty && !verified_current && verify_rearmed;

    // 未承認 gate の顔ぶれ署名 (open 来歴 ID を整列して連結)。前回促した時から変わったかを見る。
    // 同じ顔ぶれが続く間は黙り、人間待ちのゲートを毎ターン蒸し返さない。
    let gate_signature = (open_gates > 0).then(|| open_ids.join(","));
    let last_gates = read_gate_signature(input.cwd.as_deref(), input.session_id.as_deref());
    let gates_changed = open_gates > 0 && gate_signature != last_gates;

    match owox_core::stop_decision(
        input.stop_hook_active,
        open_gates,
        gates_changed,
        dirty,
        verified_current,
        verify_rearmed,
    ) {
        // 受理: 出力無しで終了 (素通り)。
        owox_core::StopDecision::Accept => ExitCode::SUCCESS,
        owox_core::StopDecision::Continue { reason } => {
            // checklist を促したなら今の verify 署名を覚え、verify.run が走るまで再び促さない。
            if want_checklist {
                remember_stop_verify_marker(
                    input.cwd.as_deref(),
                    input.session_id.as_deref(),
                    verify_sig_str,
                );
            }
            // gate の顔ぶれが変わって促したなら今の署名を覚え、同じ顔ぶれの間は黙る。
            if gates_changed && let Some(sig) = &gate_signature {
                remember_gate_signature(input.cwd.as_deref(), input.session_id.as_deref(), sig);
            }
            let output = StopContinue {
                decision: "block",
                reason: reason.clone(),
            };
            match serde_json::to_string(&output) {
                Ok(json) => {
                    println!("{json}");
                    ExitCode::SUCCESS
                }
                // JSON を出せない時は継続意図を代替契約 (終了コード 2 + stderr) で伝える。
                Err(_) => {
                    eprintln!("{reason}");
                    ExitCode::from(2)
                }
            }
        }
    }
}

/// 床コンテキストを additionalContext で注入する。
///
/// AGENTS.md 廃止で床は hook 注入のみ。SessionStart は source=startup だけでなく compact / resume でも
/// 再発火するので (hooks.json の matcher で拾う)、圧縮・再開後もこの注入で床が戻る。PostCompact は
/// additionalContext 非対応で context 注入できないため使わない (実機確認)。
/// 正本が無い・読めない場合は妨げない (黙って続行)。
fn session_start() -> ExitCode {
    let input = read_input();
    let owox = owox_dir(input.cwd.as_deref());

    if let (Some(session_id), Some(launcher_pid)) =
        (input.session_id.as_deref(), crate::cache::launcher_pid())
    {
        crate::cache::write_launcher_session(&owox, launcher_pid, session_id);
    }

    // 自動承認の窓はセッション限り。新しいセッションの開始 (startup|resume|compact) ごとに閉じ、
    // 人間が毎回明示的に開け直す (`docs/decisions/20260619-承認と自動改善ループ.md`)。
    crate::cache::close_auto_window(&owox);

    let canon = match owox_core::load_canon(&owox) {
        Ok(canon) => canon,
        Err(err) => {
            eprintln!("owox hook session-start: 正本を読めない: {err}");
            return ExitCode::SUCCESS;
        }
    };

    // 床は薄い地図だけを入れる。任務と current pressure は mcp 側で live 計算する。
    let mut context = owox_core::floor_context(&canon);
    let mission = input
        .session_id
        .as_deref()
        .map(|sid| crate::cache::mission_for_session(&owox, sid))
        .unwrap_or(crate::cache::Mission::Work);
    context.push_str(&format!("Current mission: {}.\n\n", mission.as_str()));
    context.push_str(&current_pressure_line(&owox, &canon));
    if let Ok(selection) = owox_core::select_delivery_for_phase(
        &owox,
        owox_core::DeliveryRequest::session_start(canon.state.phase),
        canon.state.phase,
    ) {
        let block = owox_core::render_delivery_block(&selection);
        if !block.is_empty() {
            context.push('\n');
            context.push_str(&block);
        }
    }
    context.push_str(
        "Use context scope=\"codebase\" when you need a repo map before choosing files to read.\n",
    );
    let output = HookOutput {
        hook_specific_output: AdditionalContextOutput {
            hook_event_name: "SessionStart",
            additional_context: context,
        },
    };

    match serde_json::to_string(&output) {
        Ok(json) => {
            println!("{json}");
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("owox hook session-start: 出力生成に失敗: {err}");
            ExitCode::SUCCESS
        }
    }
}

/// cwd から正本ディレクトリ `.owox/` を組む。cwd が無ければカレント基準。
fn owox_dir(cwd: Option<&str>) -> PathBuf {
    match cwd {
        Some(cwd) => PathBuf::from(cwd).join(".owox"),
        None => PathBuf::from(".owox"),
    }
}

/// 正本を読む。読めなければ None (呼び手は既定動作へ退避する)。
fn load_canon_from(cwd: Option<&str>) -> Option<owox_core::Canon> {
    owox_core::load_canon(&owox_dir(cwd)).ok()
}

fn current_pressure_line(owox: &Path, canon: &owox_core::Canon) -> String {
    let decisions = owox_core::list_decisions(owox).unwrap_or_default();
    let tasks = owox_core::list_tasks(owox).unwrap_or_default();
    let open = decisions
        .iter()
        .filter(|d| d.status == owox_core::DecisionStatus::Open)
        .count();
    let ready = tasks
        .iter()
        .filter(|t| owox_core::is_ready(t, &tasks))
        .count();
    let stale = owox_core::run_decay(
        &tasks,
        &decisions,
        &canon.quality.decay,
        &crate::clock::today_utc(),
    )
    .len();
    format!(
        "Current pressure: {open} open decisions, {ready} ready tasks, {stale} stale items.\n\n"
    )
}

/// 作業ツリーに verify 対象の変更があるか。.owox 配下 (来歴・タスク・キャッシュ) は除外する。
///
/// 記録ファイルだけのターンは verify 対象でないので汚れとみなさない。git が無い・読めない時は
/// 汚れ扱いにして従来どおり継続させ、変更の見逃しを避ける (安全側)。
fn working_tree_dirty(cwd: Option<&str>) -> bool {
    let dir = cwd.unwrap_or(".");
    // 包含 pathspec (`.`) を併記する。exclude だけだと git が pathspec を拒む。
    let output = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["status", "--porcelain", "--", ".", ":(exclude).owox"])
        .output();
    match output {
        Ok(out) if out.status.success() => {
            // porcelain の各行は `XY <path>` (status 2 文字 + 空白)。その形の行が 1 つでも
            // あれば変更あり。空行や想定外の行は数えない (entry 形式だけを汚れとみなす)。
            String::from_utf8_lossy(&out.stdout)
                .lines()
                .any(|l| l.as_bytes().get(2) == Some(&b' '))
        }
        // git 無し・pathspec 非対応・失敗時は汚れ扱い (見逃さない)。
        _ => true,
    }
}

/// `.owox/.cache/`。用語注入・Stop 署名の session キャッシュを置く。
fn cache_dir(cwd: Option<&str>) -> PathBuf {
    crate::cache::dir(&owox_dir(cwd))
}

/// session_id をファイル名に使える文字へ整える。
fn safe_session_id(session_id: &str) -> String {
    session_id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// session ごとの注入済み用語ファイル。
fn session_terms_path(cwd: Option<&str>, session_id: &str) -> PathBuf {
    cache_dir(cwd)
        .join("sessions")
        .join(format!("{}.json", safe_session_id(session_id)))
}

/// session ごとの Stop 再武装マーカ。前回 checklist を促した時の verify 署名を保つ。
fn session_stop_path(cwd: Option<&str>, session_id: &str) -> PathBuf {
    cache_dir(cwd)
        .join("sessions")
        .join(format!("{}-stop.json", safe_session_id(session_id)))
}

/// session ごとの未承認 gate 署名ファイル。前回促した時の open gate の顔ぶれを保つ。
fn session_gate_path(cwd: Option<&str>, session_id: &str) -> PathBuf {
    cache_dir(cwd)
        .join("sessions")
        .join(format!("{}-gates.json", safe_session_id(session_id)))
}

/// 前回 Stop で checklist を促した時の verify 署名。促していない (ファイル無し)・読めない時は None。
/// None は「まだ一度も促していない」を表し再武装扱いになる。verify 未実行時に促した記録は空文字列で残る。
fn read_stop_verify_marker(cwd: Option<&str>, session_id: Option<&str>) -> Option<String> {
    let sid = session_id?;
    std::fs::read_to_string(session_stop_path(cwd, sid))
        .ok()
        .and_then(|s| serde_json::from_str::<String>(&s).ok())
}

/// 今回 checklist を促した時の verify 署名を session キャッシュへ保存。session_id が無い・書けない時は何もしない。
fn remember_stop_verify_marker(cwd: Option<&str>, session_id: Option<&str>, signature: &str) {
    let Some(sid) = session_id else {
        return;
    };
    ensure_cache_ignored(cwd);
    let path = session_stop_path(cwd, sid);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string(signature) {
        let _ = std::fs::write(&path, json);
    }
}

/// 前回 Stop で促した時の未承認 gate 署名。session_id が無い・読めない時は None。
fn read_gate_signature(cwd: Option<&str>, session_id: Option<&str>) -> Option<String> {
    let sid = session_id?;
    std::fs::read_to_string(session_gate_path(cwd, sid))
        .ok()
        .and_then(|s| serde_json::from_str::<String>(&s).ok())
}

/// 今回促した未承認 gate 署名を session キャッシュへ保存。session_id が無い・書けない時は何もしない。
fn remember_gate_signature(cwd: Option<&str>, session_id: Option<&str>, signature: &str) {
    let Some(sid) = session_id else {
        return;
    };
    ensure_cache_ignored(cwd);
    let path = session_gate_path(cwd, sid);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string(signature) {
        let _ = std::fs::write(&path, json);
    }
}

/// この session で AI がファイルを編集したことの印。訂正検知の決定論シグナルの土台
/// (`docs/decisions/20260619-承認と自動改善ループ.md`)。
fn session_edited_path(cwd: Option<&str>, session_id: &str) -> PathBuf {
    cache_dir(cwd)
        .join("sessions")
        .join(format!("{}-edited", safe_session_id(session_id)))
}

/// この session で訂正 nudge を既に出したことの印。一度きりにして連投ノイズを防ぐ。
fn session_corr_nudge_path(cwd: Option<&str>, session_id: &str) -> PathBuf {
    cache_dir(cwd)
        .join("sessions")
        .join(format!("{}-corr-nudge", safe_session_id(session_id)))
}

/// session キャッシュへ空ファイルの印を 1 つ置く。session_id が無い・書けない時は何もしない。
fn touch_marker(cwd: Option<&str>, path: PathBuf) {
    ensure_cache_ignored(cwd);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(&path, "");
}

/// この session で AI が編集した印を立てる。
fn mark_session_edited(cwd: Option<&str>, session_id: Option<&str>) {
    if let Some(sid) = session_id {
        touch_marker(cwd, session_edited_path(cwd, sid));
    }
}

/// この session で AI が編集したか。session_id が無い時は false。
fn session_edited(cwd: Option<&str>, session_id: Option<&str>) -> bool {
    session_id
        .map(|sid| session_edited_path(cwd, sid).exists())
        .unwrap_or(false)
}

/// この session で訂正 nudge を出したか。session_id が無い時は true (= 出さない側へ寄せる)。
fn corr_nudged(cwd: Option<&str>, session_id: Option<&str>) -> bool {
    session_id
        .map(|sid| session_corr_nudge_path(cwd, sid).exists())
        .unwrap_or(true)
}

/// 訂正 nudge を出した印を立てる。
fn mark_corr_nudged(cwd: Option<&str>, session_id: Option<&str>) {
    if let Some(sid) = session_id {
        touch_marker(cwd, session_corr_nudge_path(cwd, sid));
    }
}

/// この session で注入済みの用語名 (小文字化)。session_id が無い・読めない時は空集合
/// (= 全て未注入扱い。従来どおり毎回注入へ退避する)。
fn read_injected_terms(cwd: Option<&str>, session_id: Option<&str>) -> HashSet<String> {
    let Some(sid) = session_id else {
        return HashSet::new();
    };
    std::fs::read_to_string(session_terms_path(cwd, sid))
        .ok()
        .and_then(|s| serde_json::from_str::<Vec<String>>(&s).ok())
        .map(|v| v.into_iter().collect())
        .unwrap_or_default()
}

/// 今回注入した用語名を session キャッシュへ足す。session_id が無い・書けない時は何もしない
/// (作業を妨げない)。キャッシュは git に乗せないよう `.owox/.gitignore` で除外する。
fn remember_terms(cwd: Option<&str>, session_id: Option<&str>, new_terms: &[String]) {
    let Some(sid) = session_id else {
        return;
    };
    if new_terms.is_empty() {
        return;
    }
    ensure_cache_ignored(cwd);
    let path = session_terms_path(cwd, sid);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let mut set = read_injected_terms(cwd, session_id);
    set.extend(new_terms.iter().cloned());
    let mut all: Vec<String> = set.into_iter().collect();
    all.sort();
    if let Ok(json) = serde_json::to_string(&all) {
        let _ = std::fs::write(&path, json);
    }
}

/// `.owox/.gitignore` に `.cache/` を冪等に足す。キャッシュを履歴へ乗せない。
fn ensure_cache_ignored(cwd: Option<&str>) {
    crate::cache::ensure_ignored(&owox_dir(cwd));
}

/// 読取前に軽く走査する本文。repo 内の text file だけ、各 file 8KB まで。
fn glossary_scan_texts(input: &HookInput) -> Vec<owox_core::GlossaryScanText> {
    let Some(work_dir) = input.cwd.as_deref().map(PathBuf::from) else {
        return Vec::new();
    };
    let paths = glossary_scan_targets(input, &work_dir);
    let mut texts = Vec::new();
    for path in paths {
        if let Some(text) = read_text_prefix(&path, 8 * 1024) {
            let rel = path
                .strip_prefix(&work_dir)
                .ok()
                .map(|path| path.to_string_lossy().to_string())
                .unwrap_or_else(|| path.to_string_lossy().to_string());
            texts.push(owox_core::GlossaryScanText { path: rel, text });
        }
    }
    texts
}

fn glossary_scan_targets(input: &HookInput, work_dir: &Path) -> Vec<PathBuf> {
    let tool_name = input.tool_name.as_deref().unwrap_or_default();
    if is_read_tool(tool_name)
        && let Some(path) = input
            .tool_input
            .as_ref()
            .and_then(|t| t.file_path.as_deref())
    {
        return repo_local_paths(work_dir, &[path.to_string()]);
    }
    if tool_name != "Bash" {
        return Vec::new();
    }
    let command = input
        .tool_input
        .as_ref()
        .and_then(|t| t.command.as_deref())
        .unwrap_or_default();
    repo_local_paths(work_dir, &bash_read_targets(command))
}

fn bash_read_targets(command: &str) -> Vec<String> {
    let tokens: Vec<String> = command
        .split_whitespace()
        .map(|t| t.trim_matches(['"', '\'', '`']).to_string())
        .collect();
    let Some(program) = tokens.first().map(String::as_str) else {
        return Vec::new();
    };
    match program {
        "cat" => tokens
            .iter()
            .skip(1)
            .filter(|t| !t.starts_with('-'))
            .cloned()
            .collect(),
        "head" | "tail" => {
            let mut out = Vec::new();
            let mut skip_next = false;
            for token in tokens.iter().skip(1) {
                if skip_next {
                    skip_next = false;
                    continue;
                }
                if token == "-n" {
                    skip_next = true;
                    continue;
                }
                if token.starts_with('-') {
                    continue;
                }
                out.push(token.clone());
            }
            out
        }
        "sed" => {
            let mut seen_script = false;
            let mut out = Vec::new();
            let mut skip_next = false;
            for token in tokens.iter().skip(1) {
                if skip_next {
                    skip_next = false;
                    continue;
                }
                if token == "-n" || token == "-e" {
                    skip_next = token == "-e";
                    continue;
                }
                if token.starts_with('-') {
                    continue;
                }
                if !seen_script {
                    seen_script = true;
                    continue;
                }
                out.push(token.clone());
            }
            out
        }
        _ => Vec::new(),
    }
}

fn repo_local_paths(work_dir: &Path, raw_paths: &[String]) -> Vec<PathBuf> {
    raw_paths
        .iter()
        .filter(|p| !p.is_empty() && !reads_canon_path(p))
        .filter_map(|path| {
            let joined = if Path::new(path).is_absolute() {
                PathBuf::from(path)
            } else {
                work_dir.join(path)
            };
            let canon = joined.canonicalize().ok()?;
            canon.starts_with(work_dir).then_some(canon)
        })
        .collect()
}

fn pre_tool_use_delivery_paths(input: &HookInput) -> Vec<String> {
    let tool_name = input.tool_name.as_deref().unwrap_or_default();
    let cwd = input.cwd.as_deref().map(PathBuf::from);
    match tool_name {
        "Read" | "Open" | "Edit" | "Write" | "MultiEdit" => input
            .tool_input
            .as_ref()
            .and_then(|t| t.file_path.as_deref())
            .map(|p| vec![relativize_input_path(cwd.as_deref(), p)])
            .unwrap_or_default(),
        "apply_patch" => input
            .tool_input
            .as_ref()
            .and_then(|t| t.command.as_deref())
            .map(|patch| {
                owox_core::parse_patch_changes(patch)
                    .into_iter()
                    .map(|c| relativize_input_path(cwd.as_deref(), &c.path))
                    .collect()
            })
            .unwrap_or_default(),
        "Bash" => input
            .tool_input
            .as_ref()
            .and_then(|t| t.command.as_deref())
            .map(|cmd| {
                bash_read_targets(cmd)
                    .into_iter()
                    .map(|p| relativize_input_path(cwd.as_deref(), &p))
                    .collect()
            })
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

fn edits_rules_file(input: &HookInput) -> bool {
    let tool_name = input.tool_name.as_deref().unwrap_or_default();
    let cwd = input.cwd.as_deref().map(PathBuf::from);
    match tool_name {
        "Edit" | "Write" | "MultiEdit" | "Read" | "Open" => input
            .tool_input
            .as_ref()
            .and_then(|t| t.file_path.as_deref())
            .map(|p| is_rules_path(&relativize_input_path(cwd.as_deref(), p)))
            .unwrap_or(false),
        "apply_patch" => input
            .tool_input
            .as_ref()
            .and_then(|t| t.command.as_deref())
            .map(|patch| {
                owox_core::parse_patch_changes(patch)
                    .iter()
                    .any(|c| is_rules_path(&relativize_input_path(cwd.as_deref(), &c.path)))
            })
            .unwrap_or(false),
        "Bash" => input
            .tool_input
            .as_ref()
            .and_then(|t| t.command.as_deref())
            .map(|cmd| {
                owox_core::write_targets(cmd)
                    .iter()
                    .map(|p| relativize_input_path(cwd.as_deref(), p))
                    .any(|p| is_rules_path(&p))
            })
            .unwrap_or(false),
        _ => false,
    }
}

fn pre_tool_use_delivery_operations(
    input: &HookInput,
    delivery_paths: &[String],
) -> Vec<owox_core::DeliveryOperation> {
    let mut ops = Vec::new();
    let tool_name = input.tool_name.as_deref().unwrap_or_default();
    match tool_name {
        "Read" | "Open" => push_unique(&mut ops, owox_core::DeliveryOperation::Read),
        "Edit" | "Write" | "MultiEdit" => push_unique(&mut ops, owox_core::DeliveryOperation::Edit),
        "apply_patch" => {
            push_unique(&mut ops, owox_core::DeliveryOperation::Edit);
            if input
                .tool_input
                .as_ref()
                .and_then(|t| t.command.as_deref())
                .map(|patch| {
                    owox_core::parse_patch_changes(patch)
                        .iter()
                        .any(|c| c.op == owox_core::PatchOp::Delete)
                })
                .unwrap_or(false)
            {
                push_unique(&mut ops, owox_core::DeliveryOperation::Delete);
            }
        }
        "Bash" => {
            let command = input
                .tool_input
                .as_ref()
                .and_then(|t| t.command.as_deref())
                .unwrap_or_default();
            if !bash_read_targets(command).is_empty() {
                push_unique(&mut ops, owox_core::DeliveryOperation::Read);
            }
            if owox_core::is_git_commit(command) {
                push_unique(&mut ops, owox_core::DeliveryOperation::Commit);
            }
            if is_delete_command(command) {
                push_unique(&mut ops, owox_core::DeliveryOperation::Delete);
            }
        }
        _ => {}
    }
    for op in path_derived_operations(delivery_paths) {
        push_unique(&mut ops, op);
    }
    ops
}

fn relativize_input_path(cwd: Option<&Path>, path: &str) -> String {
    let raw = PathBuf::from(path.trim_matches(['"', '\'', '`']));
    if let (Some(cwd), true) = (cwd, raw.is_absolute())
        && let Ok(rel) = raw.strip_prefix(cwd)
    {
        return rel.to_string_lossy().replace('\\', "/");
    }
    raw.to_string_lossy().replace('\\', "/")
}

fn is_delete_command(command: &str) -> bool {
    let tokens: Vec<&str> = command.split_whitespace().collect();
    matches!(tokens.first().copied(), Some("rm"))
        || matches!(tokens.first().copied(), Some("git")) && tokens.get(1) == Some(&"rm")
}

fn path_derived_operations(paths: &[String]) -> Vec<owox_core::DeliveryOperation> {
    let mut out = Vec::new();
    for path in paths {
        if is_dependency_path(path) {
            push_unique(&mut out, owox_core::DeliveryOperation::DependencyChange);
        }
        if path.starts_with(".owox/requirements/") {
            push_unique(&mut out, owox_core::DeliveryOperation::RequirementChange);
        }
        if path.starts_with(".owox/skills/") {
            push_unique(&mut out, owox_core::DeliveryOperation::SkillChange);
        }
        if path.starts_with(".owox/") && !path.starts_with(".owox/skills/") {
            push_unique(&mut out, owox_core::DeliveryOperation::CanonChange);
        }
    }
    out
}

fn is_dependency_path(path: &str) -> bool {
    matches!(
        path,
        "Cargo.toml"
            | "Cargo.lock"
            | "package.json"
            | "package-lock.json"
            | "pnpm-lock.yaml"
            | "pyproject.toml"
            | "requirements.txt"
            | "go.mod"
            | "go.sum"
    )
}

fn is_rules_path(path: &str) -> bool {
    path == ".owox/rules.md" || path.ends_with("/.owox/rules.md")
}

fn push_unique<T: PartialEq>(items: &mut Vec<T>, item: T) {
    if !items.contains(&item) {
        items.push(item);
    }
}

fn read_text_prefix(path: &Path, max_bytes: usize) -> Option<String> {
    let bytes = std::fs::read(path).ok()?;
    if bytes.contains(&0) {
        return None;
    }
    let head = &bytes[..bytes.len().min(max_bytes)];
    Some(String::from_utf8_lossy(head).into_owned())
}

fn reads_canon_path(path: &str) -> bool {
    let norm = path.trim_matches(['"', '\'', '`']).replace('\\', "/");
    (norm == ".owox" || norm.contains(".owox/")) && !norm.contains(".owox/skills/")
}

/// stdin を読み、hook 入力 JSON を解釈する。空・不正なら既定値。
fn read_input() -> HookInput {
    let mut buf = String::new();
    if std::io::stdin().read_to_string(&mut buf).is_err() || buf.trim().is_empty() {
        return HookInput::default();
    }
    serde_json::from_str(&buf).unwrap_or_default()
}
