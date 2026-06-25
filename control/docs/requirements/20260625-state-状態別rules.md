# 状態別 rules

## 状態

草案。次回以降に詳細を詰める。

## 背景

`phase = initial / stable / maintenance` は既にあり、ゲートの厳しさと AI の振る舞い案内を変えている。
別の state 軸を足すと `profile`・`phase`・`layer` と重なり、AI も人間も迷う。

新しい state 軸は増やさず、既存 phase に状態別 rules 表示を足す。

## 目的

- phase ごとの project 固有 rules を届ける
- `SessionStart` の振る舞い案内を project 固有にできる
- gate の厳しさと文脈案内を混ぜない
- 新規 tool を増やさない

## 対象

- rules 正本
- `SessionStart`
- `rules.lookup`
- `PreToolUse`
- `verify.run`

## 対象外

- 新しい state 軸
- phase ごとの別ファイル
- 散文 rules の意味的な機械判定
- `state.set` の作り直し

## 仕様

rules に phase 条件を持たせる。

```text
## Common

- 常に効く rule

## Initial

- 大きい構造変更を許す
- 既存挙動維持より設計の清潔さを優先

## Stable

- 変更量と安定性のバランスを見る

## Maintenance

- 小さく可逆にする
- 修正には回帰テストを足す
```

2層にする。

- common rules
  - 常に効く
  - 削除・依存・安全・不可逆など

- phase rules
  - phase ごとに効く
  - AI の振る舞い案内中心

## 実現方針

- `SessionStart`
  - 現在 phase の rules だけ注入する
  - 他 phase の rules は出さない

- `rules.lookup`
  - 引数なしなら common + 現在 phase の rules を返す
  - 必要なら全体取得を許す

- `PreToolUse`
  - 編集前 policy push に現在 phase の rules を含める

- `verify.run`
  - phase rules 違反を機械検出できるものだけ報告する
  - 散文の意味違反は扱わない

役割を分ける。

- 文脈: phase rules
- ゲート: phase enforcement
- プロジェクト性質: profile
- 層別権限: quality.toml layers

## 編集

- `state.set` は既存のまま
- `rules.lookup` を拡張する
- `canon.add` / `canon.propose` で rules を編集する
- 新規 tool は作らない

## 検証

- `SessionStart` が common + 現在 phase rules だけを注入する
- `rules.lookup` が common + 現在 phase rules を返す
- 全体取得で他 phase rules も返せる
- `PreToolUse` の policy push に現在 phase rules が入る
- `phase_enforcement` の既存挙動を壊さない
- `profile` や layer と混ざらない

## 未決事項

- rules.md の正確な書式
- `rules.lookup` の引数
- phase rules の機械検査対象を作るか
- `maintenance` 以外で commit に効かせる項目を許すか
