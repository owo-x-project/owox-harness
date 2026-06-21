---
name: handoff
description: Use when creating a markdown handoff document for another session under docs/handoff with filenames like YYYYMMDD-Japanese-title.md. The directory must stay ignored by git.
---

# handoff

別セッションへ作業を引き継ぐ文書を作る。

## 保存先

- `docs/handoff/<date>-<日本語タイトル>.md`
- `<date>` は `YYYYMMDD`
- `docs/handoff` は git 管理対象外

## 原則

- 次の担当がすぐ再開できる情報だけ書く
- 長い議論を貼らない
- 決定済み、未決、次作業を分ける
- 実行結果と未確認事項を残す
- 秘密情報を書かない

## 手順

1. 状態確認
   - 目的
   - 現在の差分
   - 作成済みコミット
   - 未完了作業
   - 確認結果

2. git 除外確認
   - `.gitignore` に `docs/handoff/` があるか確認
   - なければ追加する

3. 文書作成
   - タイトル
   - 目的
   - 現状
   - 決定事項
   - 変更済みファイル
   - 未完了
   - 確認済み
   - 未確認
   - 次の作業

## 完了

- 作成した文書
- 未完了作業
- 次の推奨 skill 1-3 件
  - `impl`: 実装再開が必要
  - `review`: 引き継ぎ前に確認する
  - `commit`: 共有前に履歴保存する
