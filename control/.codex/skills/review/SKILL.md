---
name: review
description: Use when reviewing code, plans, architecture, scope control, maintainability, future-proofing, or risk before accepting changes.
---

# review

範囲、設計、将来性、危険を確認する。

## 観点

- 目的から外れていないか
- 変更範囲が広すぎないか
- 既存構造を壊していないか
- 責務が混ざっていないか
- 依存方向が悪化していないか
- 将来の変更に耐えるか
- その場しのぎがないか
- 危険な失敗状態がないか
- 試験や確認が足りているか
- 文書更新が必要か

## 手順

1. 入力確認
   - 目的
   - 要件
   - 設計
   - 差分
   - 試験結果

2. 差分確認
   - ファイル単位
   - 振る舞い単位
   - 依存関係
   - 境界の変化

3. 指摘
   - 重大な順
   - ファイルと行を示す
   - 何が危険かを短く書く
   - 修正方向を示す

4. 判定
   - 問題なし
   - 修正推奨
   - 修正必須
   - 判断保留

## 完了

- 指摘
- 残る危険
- 確認不足
- 次の推奨 skill 1-3 件
  - `impl`: 修正が必要
  - `verify`: 確認不足がある
  - `commit`: 受け入れ可能
