# practices 冗長性の機械シグナル

## 状態

採用 (実装済)。Phase8 実機検証 (`docs/validation/20260617-Phase8-対話検証と粗の是正.md`) で見つけた、似た practice が無検査で溜まる問題への対処。

## 背景

`canon.add target=practices` は practices.md へ `- 日付: text` を無条件 append していた (重複チェックゼロ・完全一致すら弾かない)。実機検証で AI が、既存とほぼ同旨の practice を、それが床コンテキストに注入され目前にあったのに気づかず追加した。

問題は2つ:

- 完全一致でも別行が増える
- 似た (言い回し違い) practice が溜まる。practices は床コンテキストへ全件注入されるので、溜まるほど床が膨らみ最小コンテキスト旗に直撃する

「AI が気づいて避ける」に頼るのは弱い (routine 検知を機械化したのと同じ教訓)。鮮度 decay は「古さ」だけ見て「重複」を見ない。

## 採用設計

owox の型 (機械検出して助言・固定化の判断は人間) に載せる。2段:

### 完全一致ガード (practices.rs)

`practices::add` は append 前に、トリム後テキストが既存 entry と完全一致するなら追加しない。封筒は ok で「既に記録済み・重複は足さない」と返す (来歴も増やさない)。安い即時の安全網。

### 冗長性の advisory シグナル (decay.rs)

`run_practice_redundancy` を新設 (run_practice_decay と別関数・run_decay 署名は不変)。

- practice 対をすべて比べ、字 n-gram (3-gram) の Jaccard 類似度が閾値超えなら kind="redundant-practice" を報告
- 字 n-gram は言語非依存 (日本語も英語も・空白分かち書きに依存しない)・決定論 (LLM 不要)・新規依存なし
- 対は新しい側 (date 大) を subject にし「旧 practice と N% 似ている。canon.propose で統合を検討」と促す
- is_structural()=false (commit を止めない・助言)
- 閾値は quality.toml `[decay]` に practice_similarity (既定 0.5)。DecayConfig・DecayRaw へ

統合 (削除/置換) は既存の canon.propose (人間ゲート) へ流す。owox は気づきを機械化するだけで、間引きの判断は人間が握る。

### 報告

next と verify.run の decay 集約へ run_practice_redundancy を合流 (run_practice_decay と同じ並べ方)。

## 変更境界

触る: crates/core/src/practices.rs (完全一致ガード)・decay.rs (run_practice_redundancy)・quality.rs (practice_similarity)・crates/mcp/src/serve.rs (next/verify.run へ合流)・docs。

触らない: run_decay / run_verify の署名・practices の符号化・canon.propose の振る舞い。

## 危険

- 字 3-gram Jaccard は素朴 → 言い回しが大きく違う重複は閾値下で漏れる。advisory なので実害小・実機で閾値調整
- 短い practice 同士が偶然高類似 → 閾値 0.5 で緩めに始め調整
- 対の総当たりは O(n^2) だが practices は少数 (人間が間引く前提) なので問題なし

## 検証方針

- 単体: 完全一致 add は増えない、字 n-gram Jaccard の値、閾値超え対が redundant-practice、似ていない対は出ない、is_structural=false
- 結合: next / verify.run に redundant-practice が出る
- 手動: 似た practice を2件入れて next が統合を促す

## 見直し条件

- 字 n-gram の素朴さで漏れ・誤検出が実利用で問題になった時 (索引や別指標を検討)
- practice_similarity 既定が実利用で合わない時
