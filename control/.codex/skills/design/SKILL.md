---
name: design
description: Use before implementation when architecture, responsibility boundaries, dependency direction, extensibility, or cleanliness must be decided.
---

# design

実装前に構造を固める。

## 原則

- その場しのぎ禁止
- 責務を小さく分ける
- 依存方向を単純にする
- 将来の変更点を先に見る
- 既存構造を尊重する
- 新しい抽象は、重複や複雑さを確実に減らす場合だけ作る

## 手順

1. 入力確認
   - `AGENTS.md`
   - 関連 `INDEX.md`
   - 要件文書
   - 既存実装

2. 構造把握
   - 主要な責務
   - 既存の依存方向
   - 変更対象
   - 変更の波及

3. 設計案作成
   - 最小案
   - 将来性重視案
   - 採用案
   - 不採用理由

4. 実装境界決定
   - 触るファイル
   - 触らないファイル
   - 守る振る舞い

5. 検証方針
   - 単体確認
   - 結合確認
   - 手動確認
   - 未確認として残すもの

## 完了

- 採用設計
- 変更境界
- 危険
- 次の推奨 skill 1-3 件
  - `impl`: 実装へ進む
  - `review`: 設計妥当性を確認する
  - `docs`: 設計判断を文書へ残す
