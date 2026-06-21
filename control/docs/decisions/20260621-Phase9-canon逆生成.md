# canon 逆生成

採用判断。2026-06-21。
Phase9 要件 C群「既存コードからの正本逆生成」の残りを実装する。性質4軸の逆生成 (profile.detect) は
Phase9 既存スライスで実装済。本書は性質検出を超えて、既存 repo から rules / quality の初期案を出す
逆生成を扱う (`docs/decisions/20260611-方向付け.md` の kickoff 拡張)。

## 背景

既存プロジェクトへ後から owox を入れる時、人間が rules (不可逆操作) と quality (層・層境界) を
ゼロから書くのは負担。コードには既に手がかり (層ディレクトリ・migrations・terraform 等) があるので、
そこから初期案を逆生成して人間の出発点を作る。profile.detect が性質を draft で出すのと同じ思想。

## 採用した案

### canon.detect = rules / quality の初期案を逆生成 (proposal のみ)

- core `crates/core/src/canon_detect.rs` の `detect_canon_draft(&DetectSignals) -> CanonDraft`。
  DetectSignals は profile.detect と共有 (files・has_quality_layers・has_version_tags)
- 層: core 系ディレクトリ (domain/entities/usecase/usecases/ports/core) は guarded・端系
  (infra/infrastructure/adapters/adapter) は free で `[[layers]]` 案。glob は `**/<dir>/**` で深さ非依存
- 層境界: core と端が両在する時、core が端ディレクトリ名を参照しない方向境界を `[[boundaries]]` 案。
  forbid は端名の語境界一致正規表現 (言語非依存の起点・人間が言語の import 構文へ寄せる)
- 不可逆: 痕跡から守るべき破壊的コマンドの detect 案。migrations → migrate down/reset/drop、
  terraform/*.tf → terraform apply/destroy、k8s/helm → kubectl delete / helm uninstall
- 出力は draft + 根拠 + 貼れる断片 (render_quality_toml / render_rules_markdown)。MCP tool
  `canon.detect` が返す。何も書かない (profile.detect と同じ人間ゲート)
- 生成した detect / forbid 正規表現は妥当性検証してから返す (壊れた案を出さない)

### 別 tool として置く

profile.detect (性質4軸→profile.set) と canon.detect (guardrail draft→canon 編集) は出力型も
後続アクションも別なので別 tool にする。profile.detect の契約を汚さず、kickoff が2つの逆生成入口を持つ。
MCP コンテキスト削減の精神は守る: canon.detect は引数なしの kickoff 時低頻度 tool で schema は最小。

### 発見性を導線へ

床 routing と kickoff コマンド本文へ canon.detect を載せ、既存 repo 採用時に逆生成→人間提案の
流れへ AI を導く (発見性は床 routing が駆動・get_info 任せにしない)。

## 採用理由

- 安いファイル名シグナルで言語非依存。重い解析を持ち込まず初期案を出せる
- proposal のみで人間ゲート: その場しのぎで canon を勝手に書かない・誤検出を人間が捨てられる
- 貼れる TOML / markdown 断片を返す: 人間の編集コストを最小化
- profile.detect と機構を共有 (DetectSignals) し、新サブシステムを足さない

## 捨てた案

- canon.detect を自動で canon へ書く → 誤検出が正本を汚す・人間判断を奪う (proposal に留める)
- profile.detect へ rules/quality draft を相乗り → 出力型と後続アクションが混ざり AI が混乱する
- 言語ごとの import 構文を解析して境界を厳密生成 → 重く言語依存。語境界一致の起点を人間が寄せる
- migrations/terraform 等の detect を網羅的にツール別へ展開 → 過剰・陳腐化。代表パターンを起点に人間が足す

## 要実機確認 (target 検証)

- 層あり repo で canon.detect が guarded core / free edge と方向境界を出すか
- migrations / terraform / k8s 痕跡から不可逆 draft が出るか・detect 正規表現が実コマンドに当たるか
- 返した quality_toml / rules_markdown が貼って読み戻せるか・kickoff から逆生成→人間提案が回るか

## 見直し条件

- 層ディレクトリ命名が検出語彙と合わず取りこぼす → CORE/EDGE_LAYER_DIRS を拡充
- 不可逆 detect の誤検出/取りこぼしが目立つ → パターン調整・痕跡シグナルの追加
- 逆生成の射程を context / brand / glossary へ広げる要求 → CanonDraft を拡張 (同機構に乗せる)
