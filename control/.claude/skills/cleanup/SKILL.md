---
name: cleanup
description: Use when removing temporary workarounds, duplication, unclear names, stale files, or structural clutter without changing behavior.
---

# cleanup

その場しのぎ、重複、命名の乱れを整理する。

## 原則

- 振る舞いを変えない
- 目的外の機能追加をしない
- 大きく直す場合も目的を絞る
- 変更前後の確認方法を決める
- 既存の未関係差分は触らない

## 手順

1. 対象確認
   - 重複
   - 古い名前
   - 一時対応
   - 使われていないファイル
   - 依存の乱れ

2. 整理方針
   - 消す
   - 移す
   - 名前変更
   - 統合
   - 分割

3. 実行
   - 小さく進める
   - 差分を確認する
   - 振る舞い変更を混ぜない

4. 確認
   - 試験
   - 静的検査
   - 差分確認
   - 未確認事項

## 完了

- 整理内容
- 振る舞い変更の有無
- 未確認事項
- 次の推奨 skill 1-3 件
  - `verify`: 振る舞い維持を確認する
  - `review`: 構造確認が必要
  - `commit`: 整理変更を保存する
