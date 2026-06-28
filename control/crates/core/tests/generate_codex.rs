//! 縦貫通検証: 最小 brand.md → Codex 生成 + 床コンテキスト注入 → 出力確認。
//!
//! AGENTS.md は廃止し、床コンテキスト (向き付け・意図ルーティング・Vision・全体スタイル・状態) は
//! SessionStart / PostCompact hook が live 注入する。rules 本文と brand リストはオンデマンド注入。

use std::collections::HashSet;
use std::path::PathBuf;

use owox_core::{
    HookDecision, find, floor_context, load_canon, policy_injection, pre_tool_use_decision,
    render_rules_block,
};

fn fixtures() -> PathBuf {
    // crates/core/tests からリポジトリルートの fixtures へ戻る。
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures")
}

fn fixture_owox() -> PathBuf {
    fixtures().join("minimal/.owox")
}

#[test]
fn agents_md_is_not_generated() {
    // AGENTS.md は廃止。床は hook 注入のみ。
    let files = find("codex")
        .expect("codex target")
        .generate(&load_canon(&fixture_owox()).expect("正本を読める"));
    assert!(
        files.iter().all(|f| f.path != "AGENTS.md"),
        "AGENTS.md は生成しない"
    );
}

#[test]
fn codex_generates_hooks() {
    let files = find("codex")
        .expect("codex target")
        .generate(&load_canon(&fixture_owox()).expect("正本を読める"));

    let hooks = files
        .iter()
        .find(|f| f.path == ".codex/hooks.json")
        .expect("hooks.json を生成する");
    // 最上位 hooks ラッパが必須 (無いと Codex が hook を認識しない)。
    assert!(hooks.contents.contains("\"hooks\""));

    // SessionStart を繋ぐ。matcher を compact / resume へ広げ圧縮・再開後も床を戻す
    // (PostCompact は context 注入不可・実機確認)。command は owox を直接呼ぶ (薄いシェル廃止)。
    assert!(hooks.contents.contains("\"SessionStart\""));
    assert!(hooks.contents.contains("startup|resume|compact"));
    assert!(hooks.contents.contains("owox hook session-start"));

    // PostCompact は使わない (additionalContext 非対応で床を注入できない)。
    assert!(
        !hooks.contents.contains("PostCompact"),
        "PostCompact は登録しない"
    );
    assert!(!hooks.contents.contains("owox hook post-compact"));

    // PreToolUse を Bash と編集ツールで繋ぐ (不可逆 deny + 編集対象の用語・policy push)。
    assert!(hooks.contents.contains("\"PreToolUse\""));
    assert!(hooks.contents.contains("Bash"));
    assert!(hooks.contents.contains("apply_patch"));
    assert!(hooks.contents.contains("owox hook pre-tool-use"));

    // UserPromptSubmit を繋ぐ (プロンプトに出た用語定義と rules/brand の push)。
    assert!(hooks.contents.contains("\"UserPromptSubmit\""));
    assert!(hooks.contents.contains("owox hook user-prompt-submit"));

    // Stop を繋ぐ (完了前の確認。matcher 無し)。
    assert!(hooks.contents.contains("\"Stop\""));
    assert!(hooks.contents.contains("owox hook stop"));

    // 薄いシェルは廃止した。シェルファイルも実行ビットも生成しない。
    assert!(
        files.iter().all(|f| f.path != ".codex/hooks/session-start"),
        "薄いシェルは生成しない"
    );
    assert!(
        files.iter().all(|f| !f.executable),
        "実行ビット付き生成物は無い"
    );
}

#[test]
fn floor_carries_orientation_routing_and_vision() {
    let canon = load_canon(&fixture_owox()).expect("正本を読める");
    let floor = floor_context(&canon);

    // 薄い床: canon 禁止と entry map。
    assert!(floor.contains("Do not read or edit the canon"));
    assert!(floor.contains("## Entry map"));
    assert!(floor.contains("Use kickoff to orient"));
    assert!(floor.contains("Use next to see the intent gate"));
    assert!(floor.contains("rules.lookup, glossary.lookup, and practice.lookup"));
    // Vision は床に常時。
    assert!(floor.contains("## Vision"));
    assert!(floor.contains(&canon.brand.vision));

    // 向き付け (harness 由来の文) に CLI ツール名・製品ツール名は出さない (機能識別子の owox は可)。
    // Vision 以降は正本本文なので対象外 (プロジェクト都合で製品名に触れてよい)。
    let vision = floor.find("## Vision").unwrap();
    let orientation = &floor[..vision];
    assert!(!orientation.contains("AGENTS.md"));
    assert!(!orientation.contains("Codex"));
    assert!(!orientation.contains("owox-harness"));
}

#[test]
fn floor_omits_rules_brand_lists_and_style() {
    let owox = fixtures().join("withrules/.owox");
    let canon = load_canon(&owox).expect("正本を読める");
    let floor = floor_context(&canon);

    // rules 本文と style 一覧は床に出さない。
    assert!(!floor.contains("## Style"));
    assert!(!floor.contains("Prefer short, plain sentences"));
    assert!(!floor.contains("## Rules"));
    assert!(!floor.contains("## Change policy"));
    assert!(!floor.contains("Match the existing style"));
    assert!(!floor.contains("## Irreversible operations"));

    // 文脈地図も床へ流し込まない (関連時に context tool で取る)。
    assert!(!floor.contains("writing tests"));
}

#[test]
fn rules_block_renders_on_demand_without_detect() {
    let canon = load_canon(&fixtures().join("withrules/.owox")).expect("正本を読める");
    let block = render_rules_block(&canon.rules);

    // 「rules」と即分かる `## Rules` 配下へ集約し、各方針を小節にする。
    assert!(block.starts_with("## Rules"));
    assert!(block.contains("### Change policy"));
    assert!(block.contains("Match the existing style"));
    assert!(block.contains("### Irreversible operations"));
    assert!(block.contains("git push --force"));
    assert!(block.contains("### Hand back to a human"));
    assert!(block.contains("Editing the canon"));

    // detect: の正規表現は機械検出用。注入文には出さない。
    assert!(!block.contains("detect:"));
    assert!(!block.contains("\\bterraform"));
}

#[test]
fn prompt_word_pushes_rules_block() {
    // ユーザーが tool 名や「rules」を言わなくても、関連語で本文が届く。
    let canon = load_canon(&fixtures().join("withrules/.owox")).expect("正本を読める");
    let inj = policy_injection(
        &canon,
        canon.state.phase,
        "can I delete this file?",
        &HashSet::new(),
        false,
    )
    .expect("delete で rules を push");
    assert!(inj.context.contains("## Rules"));
    assert!(inj.context.contains("Generated artifacts may be deleted"));
    assert_eq!(inj.keys, vec!["policy:rules".to_string()]);

    // 無関係なプロンプトでは押し付けない (最小コンテキスト)。
    assert!(
        policy_injection(
            &canon,
            canon.state.phase,
            "just say hello",
            &HashSet::new(),
            false
        )
        .is_none()
    );
}

#[test]
fn target_irreversible_detect_denies() {
    // rules.md の detect: で足した target 固有の不可逆操作を、正本経由で検出する。
    let canon = load_canon(&fixtures().join("withrules/.owox")).expect("正本を読める");
    let entry = canon
        .rules
        .irreversible
        .iter()
        .find(|i| i.operation == "terraform destroy")
        .expect("terraform destroy エントリ");
    assert_eq!(entry.detect.as_deref(), Some(r"\bterraform\s+destroy\b"));

    let decision = pre_tool_use_decision(
        "Bash",
        Some("terraform destroy -auto-approve"),
        &canon.rules.irreversible,
    );
    assert!(matches!(decision, HookDecision::Deny { .. }));
}

#[test]
fn floor_injects_glossary_term_names() {
    let canon = load_canon(&fixtures().join("withrules/.owox")).expect("正本を読める");
    let floor = floor_context(&canon);
    // 用語一覧は出さず lookup 導線だけ残す。
    assert!(!floor.contains("## Glossary terms"));
    assert!(!floor.contains("- canon"));
    assert!(floor.contains("glossary.lookup"));
    assert!(!floor.contains("the source of truth"));
}

#[test]
fn floor_omits_rules_when_absent() {
    // rules.md が無い正本でも床が組め、rules 節は出ない。
    let canon = load_canon(&fixture_owox()).expect("正本を読める");
    let floor = floor_context(&canon);
    assert!(floor.contains("## Vision"));
    assert!(!floor.contains("## Rules"));
}

#[test]
fn context_map_loads_and_validates() {
    let canon = load_canon(&fixtures().join("withrules/.owox")).expect("正本を読める");
    let entries = &canon.context.entries;
    assert_eq!(entries.len(), 2);

    let tests = entries.iter().find(|e| e.scope == "writing tests").unwrap();
    assert_eq!(tests.kind, owox_core::ScopeKind::Task);
    assert!(tests.reads.contains(&"tests/README.md".to_string()));
    assert!(
        tests
            .notes
            .contains(&"keep tests deterministic".to_string())
    );

    let api = entries.iter().find(|e| e.scope == "src/api/").unwrap();
    assert_eq!(api.kind, owox_core::ScopeKind::Path);
}

#[test]
fn targets_load_and_validate() {
    let canon = load_canon(&fixtures().join("withrules/.owox")).expect("正本を読める");
    let codex = canon
        .targets
        .entries
        .iter()
        .find(|t| t.name == "codex")
        .expect("codex target");
    assert_eq!(codex.out_dir, ".");
    let default = codex.models.iter().find(|m| m.tier == "default").unwrap();
    assert_eq!(default.model, "claude-opus-4-8");
    assert!(codex.models.iter().any(|m| m.tier == "fast"));
}
