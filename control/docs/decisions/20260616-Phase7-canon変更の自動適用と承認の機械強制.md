# Phase7 対話検証で見つけた粗の是正: canon 変更の自動適用と承認の機械強制

## 背景

Phase7 全スライス実装・機械確証後の実機 Codex 対話検証 (`docs/validation/20260616-Phase7-対話検証と粗の是正.md`) で、canon の変更・削除フローに粗が出た。

- canon.propose は変更内容を自由文で来歴へ記録するだけ・具体的な適用可能変更を保存しない
- gate.approve は来歴 status を adopted にするだけ・canon ファイルを編集しない
- 設計は「人間が canon を手編集し gate.approve で解決」前提だった (canon.rs の旧コメント)

結果、AI が「rules の古い行を消して」で:
- canon を手編集する仕組みが無い・AI は .owox 直読み禁止
- 生成物 AGENTS.md (rules 内容を複製) を直接編集し「消した」と報告
- 正本 .owox/rules.md は無傷 → 正本/生成物の乖離 + 誤った完了報告

核心 (人間が重要判断を握りつつ AI に手間をかけさせない) を破る。

## 判断

承認 = owox が canon へ自動適用する。人間は手編集せず承認のみ。AI は canon もファイルも触らない。

### 1. 承認の機械強制 (destructive 注釈)

gate.approve に `annotations(destructive_hint = true)` を付ける (rmcp 1.7.0)。Codex は destructive 注釈つき MCP ツールを approval_policy=never でも auto モードでも必ず人間確認する (組織の requirements.toml 以外で上書き不可)。AI が変更依頼を承認権限と取り違えても、人間の確認なしに素通りできない。実機で確認プロンプト発火を確認済み。

### 2. 承認時の自動適用 (構造化 canon.propose)

canon.propose に構造化変更を足す。

- op=remove (item を 1 件削除) / op=replace (item を to へ置換)。target は brand/rules/practices/glossary、brand/rules は section
- owox が propose 時に対象見出し配下の項目と照合し、一致 1 件なら open gate に具体変更 (ProposedChange) を保存し needs_human。canon は変えない
- 不一致 or 複数一致なら failed + 現項目を data.items で返す (編集時だけ範囲限定で開示・直読み禁止は維持・AI が正確に再提案できる)
- 自由文 change のみ (op 無し) は従来どおり open gate で needs_human (単純 1 項目でない編集の逃げ道・人間が手編集)
- gate.approve は来歴に ProposedChange が紐づけば canon へ適用 (apply_pending_canon_change を先に実行・失敗なら承認しない)。紐づかない通常ゲートは従来どおり flip のみ
- propose 後に canon が変わって対象項目が消えていれば apply は failed・status は open のまま (古い前提で適用しない・安全側)

ProposedChange は来歴 Markdown の `## Proposed change` 節に保存 (別ストアを持たない方針を継承)。gate ライフサイクルは記録層 (record.rs) が汎用に持ち、canon 固有の適用は canon.rs が持つ。承認の合成は両方へ依存する mcp 側 (serve.rs) で行い循環依存を避ける。

### 3. 編集可能な canon 複製を生成物から排除 (AGENTS.md 最小化)

rules 内容が編集可能な AGENTS.md に複製されていたのが、AI が生成物を直接編集する原因。指示で「生成物を編集するな」と書いても AI は無視した (Codex の hook は Edit に発火するが、deny でなく案内だった)。

brand のリスト (Values/Principles/Non-goals/Success criteria/Style) と rules 全部 (Change/Dependency/Deletion/Safety/Irreversible/Hand back) を AGENTS.md から外し、SessionStart のライブ注入へ移す。編集できるファイル複製が存在しなくなり、AI は変更時に canon.propose を使うしかなくなる。Vision だけは向き付けのアンカーとして AGENTS.md に残す。

これは Phase5 で glossary 用語名・practices を「静的 AGENTS.md 列挙をやめ live 注入」へ移した方針 (`docs/decisions/20260613-Phase5-スキルと入口.md`) の brand/rules への延長。canon リストの描画ヘルパは codex.rs から hook.rs (session_start_context) へ移設。

### 4. deny メッセージの陳腐化是正

canon 直読み deny のメッセージが撤去済みの `owox:// resources` を案内していた (resource は実機 Codex で取りに来ず撤去済み・読みは tool 一本化)。tool 誘導 (glossary.lookup・canon.propose) へ直した。

## 変更

- core: record.rs (ProposedChange 型・Decision へ proposed_change・render/parse の `## Proposed change`・record_decision_with_change・load_decision)、canon.rs (ProposeInput・構造化 propose・apply_pending_canon_change・見出し配下の項目照合/削除/置換ヘルパ)、hook.rs (deny メッセージ是正・brand/rules を session_start_context へ注入・描画ヘルパ移設)、targets/codex.rs (AGENTS.md から brand リスト/rules を除去・Vision とオリエンテーションのみ)、lib.rs (公開)
- mcp: serve.rs (canon.propose 引数へ op/section/item/to/change・gate.approve へ destructive 注釈と apply 呼び出し)
- cargo 211 passed・clippy/fmt クリーン

## 見直し条件

- 文字列一致の項目照合が実運用で取りこぼす場合 (disclose-on-need の現項目返却で AI が再提案する想定・頻発なら正規化や曖昧一致を検討)
- 別 CLI を対象に足す時 destructive 相当の承認強制が無い場合 (CLI ごとの承認経路を再設計)
- AGENTS.md 廃止・コンテキスト配信の動的化は別改善で検討 (`docs/handoff/20260616-コンテキスト配信の再設計.md`)
