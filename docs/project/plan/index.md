# Plan

## 役割

このディレクトリは、実装計画とロードマップを管理する正本です。計画は検証可能なフェーズに分け、各フェーズは実作業単位の task に分解します。

## 置いてよいもの

- 実装計画
- ロードマップ
- フェーズ定義
- plan task
- フェーズごとの検証方針
- requirement / spec / ADR / validation / pattern への参照

## 置いてはいけないもの

- AI エージェントの作業ログ
- `.agents/tasks/` に置く文脈キャッシュ
- フェーズや task に分解されていない大きな作業メモ
- 正式化前の設計草案だけを目的とする proposal

## 構成

```text
docs/project/plan/
  index.md
  <plan-slug>/
    index.md
    phase-01-<slug>/
      index.md
      task-001-<slug>.md
      task-002-<slug>.md
    phase-02-<slug>/
      index.md
      task-001-<slug>.md
```

## 計画の単位

- 1 plan は 1 つの実装目的またはロードマップを扱う
- 1 phase は検証可能な中間成果を扱う
- 1 task は AI エージェントが 1 セッションで高品質に完了しやすい作業量にする
- 1 task は必ず 1 つの Markdown ファイルに分ける

## plan task の必須項目

- 目的
- 前提条件
- 作業内容
- 完了条件
- 検証方法
- 依存関係
- 成果物

## 命名規則

- plan ディレクトリ: `<plan-slug>/`
- phase ディレクトリ: `phase-<2桁番号>-<slug>/`
- phase index: `index.md`
- task 個票: `task-<3桁番号>-<slug>.md`

## 参照ルール

- plan 追加時はこの `index.md` に参照を追加する
- phase 追加時は plan 直下の `index.md` に参照を追加する
- task 追加時は phase 直下の `index.md` に参照を追加する
- plan task は `.agents/tasks/task-*.md` の代替にしない
- `.agents/tasks/task-*.md` は plan task の代替にしない

## 参照

- 現在は個別 plan なし: 追加時は `<plan-slug>/index.md` と phase/task 個票を置く
