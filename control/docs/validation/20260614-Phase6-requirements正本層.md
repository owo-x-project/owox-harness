# Phase6 スライス1 requirements 正本層の検証

## 検証対象

Phase6 スライス1 (`docs/decisions/20260614-Phase6-requirements正本層.md`) で実装した requirements 正本層と 6 tool。

- core: `crates/core/src/requirement.rs` (型・読込・CRUD ロジック)
- mcp: `crates/mcp/src/serve.rs` の 6 tool (requirement.create / list / get / update / add_criterion / link_verification)

検証する owox の版: 本作業ツリーの debug ビルド (`target/debug/owox`)。
release `/usr/local/bin/owox` は未更新 (Phase5 版)。

## 結果

### 単体 (cargo)

- cargo test 122 passed (113→122。requirement.rs に 9 件追加)
- clippy clean・fmt clean

### MCP stdio (実 owox serve をサンドボックス `.owox` へ接続)

target を汚さないため一時サンドボックス (`/tmp` の空 `.owox`) に対し owox serve を立て、JSON-RPC で initialize → tools/call を流して確認した。

- get_info の instructions に requirement 系 6 tool が露出する (tool 発見性。Phase4 の教訓に沿う)
- requirement.create: inline の受け入れ基準つきで作成。id=日付+slug、基準に 1 から採番
- requirement.add_criterion: 既存要件へ追加し番号 2 を自動採番
- requirement.link_verification: 基準 2 へ検証 link を張る
- requirement.list: criteria=2・unlinked=0 を返す (検証 link 欠落数の可視化)
- requirement.update (reason なしの statement 変更): failed で弾く (本質変更は来歴連動の強制)
- requirement.update (reason つき): ok。adopted 来歴を記録し要件へ decision link、来歴は要件へ requirement link で逆参照
- requirement.get: 全要件を構造化して返す (canon 直読み禁止の読み口)
- 生成ファイルは「エントリ + 属性行」方式の Markdown で round-trip する (given/when/then/verify)

### 清潔さ

- target repo (`/workspace/product/target`) は未変更 (検証はサンドボックスで実施)
- サンドボックスは検証後に削除
- control の差分は意図どおり (requirement.rs 新規・lib.rs / serve.rs / 各 INDEX 編集・設計と検証文書)

## 失敗と再現手順

なし (全観点が期待どおり)。

## 観察 (将来の見直し候補)

- 来歴 ID が長い: `20260614-Update-requirement-20260614-Login-redirect-the-statement`。decision title に要件 ID を埋めるため。機能は正常。気になれば update の来歴 title を短縮 (task.update の前例に揃えるか検討)

## 未確認事項

- 実機 Codex の対話: AI が instructions から requirement 系 tool を見つけ、自発的に create → add_criterion → link_verification → get を回すか、canon を直読みせず tool で読むか、title/statement の来歴連動が過剰に感じないか。codex exec は承認 never 固定で MCP tool 完走不可のため対話 Codex が要る (Phase5 と同条件)
- 充足の機械判定・trace 欠落検出 (スライス2)
- release バイナリ更新 (実機検証時に再ビルド+配置)

## 次の推奨

- `commit`: スライス1 を区切る
- スライス2 へ: `plan` または `design` (要件↔テスト trace + 要件完了の機械判定)
