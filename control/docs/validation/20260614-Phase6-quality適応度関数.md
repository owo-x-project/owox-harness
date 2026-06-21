# Phase6 スライス3 quality 適応度関数の検証

## 検証対象

Phase6 スライス3 (`docs/decisions/20260614-Phase6-quality適応度関数.md`)。
quality.toml の品質バーを言語非依存で機械検証する。

- core: quality.rs (Quality 型・from_toml・glob→regex・run_quality)、model.rs/load.rs (quality.toml 読込)、verify.rs (verify.run が違反を報告)、hook.rs (commit_gate に quality の phase 適応)
- mcp: files.rs (git ls-files でファイル列挙)、serve.rs/hook.rs の配線

検証する owox の版: 本作業ツリーの debug ビルド。

## 結果

### 単体 (cargo)

- cargo test 140 passed (132→140。glob 変換・行数予算・禁止パターン・正規表現検証・未知キー・commit_gate の phase 適応を追加)
- clippy clean・fmt clean

### MCP stdio / hook (サンドボックス: 層境界違反 + 行数超過を仕込む)

quality.toml に行数予算 (max_lines=3) と層境界 (src/domain は infra 参照禁止) を置き、違反ファイルを作って確認した。

- verify.run: data.quality に違反を報告 (boundary 違反を path/kind/detail で開示)。next_actions に「保守では commit を止める」助言。封筒 status と完了3区別は変えない (報告のみ)
- commit ゲート (初期 phase): allow + additionalContext で警告。2 違反 (budget・boundary) を列挙
- commit ゲート (保守 phase): deny。同じ 2 違反を理由に commit を止める
- budget (行数超過) と boundary (禁止パターン) の両方を検出
- glob (src/**/*.rs・src/domain/**) が期待どおり照合

### 線引きの確認

owox 直接検証はファイル行数予算と禁止パターンの2種に絞った。循環・複雑度は config の検査コマンドへ委譲 (owox は構文解析を持たない)。違反の判定は commit ゲートに集約し phase 適応 (保守=block・初期/安定=警告)、verify.run は常時報告。「初期=緩い・保守=厳しい・コアゲートは常時」と整合。

### 清潔さ

- target repo は未変更 (サンドボックス検証)・後片付け済み
- control 差分は意図どおり (quality.rs/files.rs 新規・model/load/verify/hook/lib/serve 編集 + 設計・検証文書)

## 失敗と再現手順

なし (全観点が期待どおり)。検証中、再ビルド前の stale バイナリで一度 data.quality が出ず空ゲートになったが、cargo build 後に解消 (実装の問題ではない)。

## 未確認事項 (後の一括対話検証へ)

- 実機 Codex の対話: quality.toml を書いて違反を仕込み、保守 phase で commit が弾かれるか、verify.run の助言で AI が違反を直すか
- スライス1・2 の観察 (来歴 ID 長) は継続
- スライス4 (plan-alignment + 多視点レビュー枠組み)
- 差分量予算・per-rule severity (将来)

## 次の推奨

- `commit`: スライス3 を区切る
- スライス4 へ: `design` (plan-alignment + 多視点レビュー枠組み。Phase8 と線引き)
