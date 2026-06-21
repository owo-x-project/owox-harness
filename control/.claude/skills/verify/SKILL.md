---
name: verify
description: Use when running checks, tests, builds, static analysis, or manual verification, and when reporting what was verified or not verified.
---

# verify

実行確認、試験、静的検査を行う。

## 原則

- 何を確認したか明記
- 何を確認していないか明記
- 失敗は原因、再現手順、次の打ち手に分ける
- 確認不能な場合は理由を残す
- 試験なしで安全と言わない

## 手順

1. 確認対象を決める
   - 変更内容
   - 影響範囲
   - 失敗しやすい点

2. 確認方法を選ぶ
   - 既存試験
   - 静的検査
   - ビルド
   - 手動確認
   - 代替確認

3. 実行
   - 既存の命令を優先
   - `README.md` や設定ファイルから命令を探す
   - 必要な依存がない場合は勝手に広げず、状況を示す

4. 結果整理
   - 成功
   - 失敗
   - 未確認
   - 次の打ち手

## 完了

- 実行した確認
- 結果
- 未確認事項
- 次の推奨 skill 1-3 件
  - `review`: 結果込みで品質確認する
  - `impl`: 失敗修正が必要
  - `commit`: 確認済み変更を保存する
