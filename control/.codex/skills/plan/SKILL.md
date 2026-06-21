---
name: plan
description: Use when requirements, scope, specifications, or implementation direction must be clarified with the user before changing code. Creates requirement documents under docs/requirements when the plan should persist.
---

# plan

要件・仕様・実現方針を固める。

## 原則

- いきなり詳細へ進まない
- 一番外側の目的から決める
- 解決したいこと、使う人、成功条件、対象外を先に固定
- 不明点は推測で埋めず、人間判断が必要な点として出す
- 小さい不明点は仮置き案を示す

## 手順

1. 現状把握
   - `AGENTS.md`
   - 関連 `INDEX.md`
   - 既存文書
   - 既存実装
   - 未追跡変更

2. 外側の要件整理
   - 目的
   - 解決したい問題
   - 利用者
   - 成功条件
   - 対象外
   - 制約

3. 詳細化
   - 仕様
   - 入出力
   - 失敗時の扱い
   - 検証方法

4. 実現方針
   - 変更対象
   - 依存関係
   - 段階分け
   - 危険
   - 人間判断が必要な点

5. 永続化
   - 必要なら `docs/requirements/REQ-<date>-<cat>-<日本語タイトル>.md` 作成
   - `<date>` は `YYYYMMDD`
   - `<cat>` は既存分類へ合わせる
   - 分類がなければ短い英数字

## 文書形式

- 背景
- 目的
- 対象
- 対象外
- 成功条件
- 仕様
- 実現方針
- 検証
- 未決事項

## 完了

- 決定事項
- 未決事項
- 次の推奨 skill 1-3 件
  - `design`: 構造判断が必要
  - `impl`: 仕様が十分固まった
  - `review`: 方針確認が必要
