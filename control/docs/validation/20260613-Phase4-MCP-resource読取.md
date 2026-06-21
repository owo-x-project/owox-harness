# Phase 4 MCP resource 読取 実機検証

## 状態

完了。`owox serve` (MCP サーバ) を立て、実機 Codex が MCP resource をモデルへ渡すことを確証。
Phase 1 で保留した「MCP resource 読取」が解消。設計の核「読みは resource へ寄せる」は Codex で成立する。

## 確証 (2026-06-13)

縦貫スライス: `owox serve` + resource 1 本 (owox://brand)。実機 Codex で 2 段階に確認。

サーバ単体 (手動 JSON-RPC over stdio):

- initialize → capabilities.resources を告知して応答
- resources/list → owox://brand を 1 件返す
- resources/read owox://brand → 型付き Canon から描画した brand 本文を text/markdown で返す
- 未知 URI → resources/read で resource_not_found
- 正本欠落・壊れ → internal_error (失敗を構造化して返す)

実機 Codex (codex exec, 非対話) — 2 回確認:

- 手置き config: `mcp: owox/read_mcp_resource` で owox://brand の Vision を一字一句引用
- 生成 config (`owox generate` 産・パスなし `args=["serve"]`): `mcp: owox/list_mcp_resources` → `owox/read_mcp_resource` で Success criteria を正しく列挙

結論: **Codex は MCP resource をモデルに渡す**。公式ドキュメントは tool しか書いていないが、実機は `list_mcp_resources` / `read_mcp_resource` ツール経由で resource を一覧・読取できる。推測 (ドキュメント沈黙) ではなく実機で成立を確認した。

生成フローでの成立も確認: `owox generate codex .` が `.codex/config.toml` へ `[mcp_servers.owox]` を出し (共有設定をマージ・他設定保持)、生成 config はパスを焼かないため owox serve が `.owox` を上方探索して正本を解決する。リポジトリを移しても動く。

## 一次情報との差 (重要)

Codex マニュアル・developers.openai.com/codex/mcp.md とも、MCP は tool しか記載が無い (resources/list・resources/read・prompts・@-mention の記述なし)。
だが実機 0.139.0 は resource を `read_mcp_resource` という組込みツールでモデルへ開示する。
→ resource 読取は「ネイティブ @-mention」ではなく「MCP resource を読むツール経由」。設計の進行的開示は成立するが、resource はモデルが能動的にツールで取りに来る形。

## 検証日時

2026-06-13

## 検証した target repo

`/workspace/product/target` (product リポジトリで gitignore 済みのサンドボックス)

## 環境

- Codex CLI 0.139.0、model gpt-5.4-mini
- owox: release ビルドを `/usr/local/bin/owox` へ配置 (PATH 上)
- MCP 実装: 公式 rust SDK rmcp 1.7.0 (server / transport-io)。serve だけ tokio runtime を起こす

## 配置した設定

`/workspace/product/target/.codex/config.toml` (owox generate 産):

- `[mcp_servers.owox]` command = owox / args = ["serve"] (stdio)。パスは焼かず serve のみ
- 生成は MergeToml: 既存の人間設定 (model 等) や他 mcp_servers を残し、owox ブロックだけ差し替え
- target は `~/.codex/config.toml` で trusted 登録済み

## 実行した確認 (機械)

- サーバ単体: 上記 5 経路を手動 JSON-RPC で確認 (initialize / list / read / 未知 URI / 正本欠落)
- 全テスト 30 通過
- `codex mcp list` / `codex mcp get owox` で Codex がサーバを enabled/stdio で認識

## 結果

スライスのゴール達成。正本 (.owox) → 型付き Canon → MCP resource → 実機 Codex のモデルが読む、まで一気通貫で成立。

## 散文 resource の重複発見と設計転換 (2026-06-13)

brand resource はパイプ実証には成功したが、人間レビューで「brand は AGENTS.md にもある→重複では」と指摘。
一意マーカー実験で「resource からしか来ない」と一度は確証 (owox://brand の Vision に AGENTS.md に無い
SENTINEL を入れ、モデルがそれを引用)。だが用語定義で再検証すると、モデルは owox://glossary を使わず
`rg` + `sed` で `.owox/brand.md` を直読みした。

結論: 正本の散文 resource は AGENTS.md だけでなく `.owox/` ファイルとも重複する。
モデルはファイル読取権を持つ限りファイルを直読みする。「読みは resource へ寄せる」は成立しない。

転換 (`docs/decisions/20260611-MCP設計.md` 改訂):

- owox://glossary (用語定義 pull) は廃止。serve の resource は owox://context (ナビ地図) に置換 (サーバ単体で確認)
- AGENTS.md の用語集は用語名だけに削減 (定義を載せない)
- 用語定義は UserPromptSubmit hook の能動 push へ (プロンプトに用語が出たら定義だけ注入)

## 用語定義 push の検証 (2026-06-13、訂正あり)

2 トリガを実装: プロンプト (UserPromptSubmit) と編集対象 (PreToolUse の apply_patch/Edit/Write)。
照合 (大文字小文字無視の部分一致) は core `glossary_injection`。用語集は brand.md から
glossary.md へ分離 (直読み時に brand の他項目を巻き込まない)。

検証:

- 単体 (決定論): user-prompt-submit / pre-tool-use とも、用語入り stdin → 定義の additionalContext、
  用語なし → 素通り。確認済み
- UserPromptSubmit: **実機発火を確認** (対話で人間が確認済み + codex exec でも発火し定義が届いた)。
  ※ 一時「exec で発火せず」と記したが、原因は install 失敗で古いバイナリを使っていたため。訂正
- PreToolUse: exec で発火する (git push --force が hook の deny 理由で止まった。effect で確認)。
  ただし exec のログに `hook: PreToolUse` 行は出ない (SessionStart/UserPromptSubmit/Stop は出る)
- 編集 push (apply_patch) の end-to-end は未確認。モデルは簡単なファイル作成を apply_patch でなく
  Bash (`printf > file`) で行い、プロンプトに用語を入れず切り分ける試行が成立しなかった

発見した隙 (要検討):

- 編集 push は apply_patch/Edit/Write 限定。**Bash リダイレクト書き込み (printf/cat > file) は素通り**。
  Bash 全走査はノイズ源なので採らなかったが、ファイル書き込みの取りこぼしが残る

## 未確認 / 次に確認すること

- 編集 push (apply_patch) の end-to-end (対話、または apply_patch を強制する手順)
- Bash 書き込みの取りこぼしへの対応方針 (走査するか、許容するか)
- owox://context をモデルが「読む先の案内」として使うか (対話)
- tool 経路 (共通返り値・needs_human の人間ゲート) は未着手。次の縦貫で確認
- 他 CLI (Claude Code 等) の resource / hook 契約は別途確認

## 付随修正

- target の `.owox/brand.md` の見出しが旧 `Goal` のままで、core の `GoalをVisionに変更` 後の parser で load 失敗していた。`Vision` へ修正 (サンドボックス正本の追従)
