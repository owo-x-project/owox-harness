# harness 庭師

## 状態

草案。次回以降に詳細を詰める。

## 背景

`target harness` は運用で育つ。用語・指針・スキル・記憶・MCP tool 説明が増え続けると、床文脈が重くなり、AI が迷う。
庭師機能は不要を自動削除するものではない。腐りそうな正本・入口・記憶を検出し、人間が剪定判断できる形へ出す。

## 目的

- `target harness` が育つほど重くなる問題を防ぐ
- 床文脈・用語・指針・スキル・MCP tool 説明が膨らむ前に気づく
- 削除や統合は人間判断へ寄せる
- 検出は機械、判断は人間、適用は既存経路へ寄せる

## 対象

- glossary
- practices
- rules
- skills
- MCP tool 説明
- `SessionStart` 床文脈
- 未解決 gate
- 古い branch memory
- 古い knowledge
- 生成物と正本のずれ

## 対象外

- 自動削除
- 根拠なしの剪定提案
- 正本変更の自動確定
- 新規 pruning 専用 tool

## 仕様

検出するもの:

- 重複: 似た practices / 用語 / rules
- 低利用: 使われない skill / 読まれない practice
- 肥大: 床文脈がしきい値超え
- 陳腐化: 古い knowledge / branch memory / open gate
- 壊れ: skill 契約違反、参照切れ、生成物ずれ
- 迷子: 要件・来歴・検証に繋がらない正本変更
- 導線不良: `SessionStart` に書いているのに AI が使わない tool

庭師機能は掃除をする機能ではなく、掃除すべき理由を構造化して出す機能。

## 実現方針

新規 tool は作らない。

- `verify.run`
  - `data.gardening` に検出結果を出す
  - 原則は助言
  - 壊れた契約・参照切れ・生成物ずれだけ失敗扱い候補

- `next`
  - 上位の剪定候補だけ出す
  - 今やるべき掃除として短く出す

- `context scope="diff"`
  - このブランチで増えた正本・入口・文脈量を出す
  - 変更が庭師観点で怪しい時だけ根拠を出す

- `review.lenses`
  - 既存の `pruning` 観点を使う
  - 新しい観点は増やさない

適用方法:

- rules / practices / glossary の変更: `canon.propose`
- practices 追加: `canon.add`
- skill の整理: 既存 skill 寿命管理へ寄せる
- gate / task / knowledge / branch memory: 既存の状態遷移や supersede へ寄せる
- 物理削除: 削除基準 + 検証 + 人間確認を通す

## 検証

- `verify.run` が庭師候補を返せる
- `next` が上位候補だけを短く返せる
- `context scope="diff"` がこのブランチで増えた文脈量を返せる
- 重複 practice / 用語を検出できる
- skill 契約違反を検出できる
- 古い knowledge / branch memory / open gate を検出できる
- 自動削除しない
- 正本変更は人間ゲートへ寄る

## 未決事項

- `data.gardening` の項目構造
- 各検出のしきい値
- 低利用の測り方
- 導線不良の測り方
- 生成物ずれの対象範囲
- どの違反を `failed` にするか
