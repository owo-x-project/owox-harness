# Phase 1 Codex 実機検証

## 状態

完了。hook 発火・additionalContext 注入を実機で確証。MCP resource 読取は Phase 4 へ分離。

## 実機確証 (2026-06-12)

スキーマ修正後の対話 Codex CLI で確認。2 つの独立証拠で成立:

- Codex が「SessionStart hook (completed)」を表示し、注入された hook context にこちらの additionalContext (北極星 + 次に読むもの + owox://context / owox://next) がそのまま出た
- 仮設置の痕跡記録が `/tmp/owox-hook-fired` へ追記された (モデル挙動に依らない物理証拠。hook シェルが実際に起動)
- AI が「次に読むもの」として owox://context / owox://next を認識

結論: 推測で作っていた hook 発火と文脈注入が実機で成立。Phase 1 のゴール達成。設計の背骨は本当に動く。

確認後、検証用の痕跡記録は撤去し target を正規生成物へ復元済み。

## 検証日時

2026-06-12 (機械検証分)

## 検証した target repo

`/workspace/product/target` (product リポジトリで gitignore 済みのサンドボックス)

## 環境

- Codex CLI 0.139.0
- owox: release ビルドを `/usr/local/bin/owox` へ配置 (PATH 上。hook シェルが `owox` を解決できる)

## 生成した target harness

`owox generate codex .` で配置:

- AGENTS.md: ルート指示ファイル (サンドボックス正本 brand を反映)
- .codex/hooks.json: SessionStart を startup で session-start シェルへ登録
- .codex/hooks/session-start: 薄いシェル (実行ビット付き)。`owox hook session-start` を呼ぶ

## 実行した確認 (機械)

- 生成: 3 件配置。session-start に実行ビット (0o755) が付く
- 冪等性: 再生成前後で全生成物の sha256 が不変
- hook 出力: `{"cwd":"<target>"}` を stdin で渡すと、Codex SessionStart 形式 JSON (`hookSpecificOutput.hookEventName=SessionStart` + `additionalContext`) を stdout へ返し終了コード 0
- 正本反映: サンドボックス固有の北極星が AGENTS.md と additionalContext の両方に出る

## 結果 (機械)

機械検証は全て通過。配管 (正本 → 生成 → 配置 → hook 補助) が target repo 上で成立。

## 実機 1 次試行の所見 (2026-06-12)

対話セッションと `codex exec` の 2 経路を試し、以下が判明。

判明した環境要因:

- グローバル `/root/.codex/AGENTS.md` が `@/root/.codex/RTK.md` を取込む (RTK = 環境の CLI プロキシ、owox と無関係)。AI の「次に読むもの = RTK.md」回答はこのグローバル設定由来
- target 由来の AGENTS.md (北極星) は対話セッションで AI に届いた → AGENTS.md ネイティブ読込は成立

判明した hook の挙動:

- `/root/.codex/config.toml` の `[hooks.state]` に信頼登録があるのは `control/.codex/hooks.json` (control harness) のみ。target の hooks.json は未登録 (未承認)
- Codex の hook は定義ハッシュ単位の信頼承認制。`codex exec` は `approval: never` で動くため、未承認の hook は承認プロンプトを出せず黙ってスキップする
- よって非対話 `codex exec` では SessionStart hook の発火を検証できない (原理的制約)

結論:

- AGENTS.md ネイティブ読込: 確証
- SessionStart hook 発火・additionalContext 注入: 未確証。TTY のある対話起動で信頼承認を経て確かめる必要がある

## 根本原因と修正 (2026-06-12)

対話起動でも hook が一切現れない (承認プロンプトすら出ない) と判明。Codex マニュアル (一次情報) と照合した結果、生成する hooks.json のスキーマ誤りが原因。

- 誤: 最上位に `{"SessionStart": [...]}` (Claude Code 由来の推測)
- 正: 最上位 `hooks` ラッパ配下にイベント名を置く `{"hooks": {"SessionStart": [...]}}`
- イベント名のキャメルケース (`SessionStart`)・matcher `startup` は正しい (マニュアル実例と一致)。`config.toml` の `[hooks.state]` がスネークkeyなのは内部正規化後のキーで、JSON のキーではない

ラッパが無いと Codex は hook 定義として認識せず、`/hooks` にも出ず承認もできない。

修正: 生成器 (crates/core/src/targets/codex.rs) にラッパを追加。テストでラッパ存在を検証。再生成で target の hooks.json も修正済み。

補足 (今後の検討、本 Phase では未対応):

- マニュアルは repo ローカル hook の command を git ルート基準で解決するよう推奨 (サブディレクトリ起動でも安定)。現状は相対 `.codex/hooks/session-start`
- 薄いシェル経由をやめ command を直接 `owox hook session-start` にすればシェルファイルと実行ビットが不要になり清潔。設計判断として別途検討

## 未確認 (対話起動 + 承認で確認)

- 実機 Codex CLI で SessionStart hook が実際に発火するか
- additionalContext の文脈が AI の会話に届くか (出力契約は一次情報未取得、推測のまま)
- 生成 hook の信頼承認フロー (定義ハッシュ単位承認) が起動時にどう出るか

## 確認手順 (人間、TTY のある実ターミナルで)

`codex exec` (このセッションの `!`) では未承認 hook がスキップされ検証不可。実ターミナルで対話起動する。

検証用に target の session-start シェルへ痕跡記録を仮設置済み (発火すると `/tmp/owox-hook-fired` へ追記。正本ではない。確認後に削除)。

1. 実ターミナルで `cd /workspace/product/target`
2. `codex` を対話起動
3. hook 定義の信頼承認を求められたら**承認** (target の session-start を信頼登録)
4. 開始直後の最初の発言: 「owox の文脈で『次に読むもの』として何が指示されている? ファイルは読まず与えられた文脈だけで答えよ」
5. 判定:
   - AI が `owox://context` / `owox://next` をファイル読込なしで答える → additionalContext 注入が成立
   - 別セッションで `/tmp/owox-hook-fired` に行が増えていれば → hook シェルが物理的に発火 (モデル挙動に依らない証拠)
6. 発火しない場合: 承認プロンプトが出たか / `/tmp/owox-hook-fired` が空か を切り分け、Codex の hook ログを確認

## 次に確認すること

- 上記実機確認の結果をこの文書へ追記
- MCP resource (owox://) 読取は owox serve 未実装のため本 Phase では未検証 (Phase 4 で serve 実装後に確認)
