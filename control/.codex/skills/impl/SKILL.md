---
name: impl
description: Use when starting implementation. The main agent plans, supervises, reviews strictly, and delegates simple or scoped work to subagents when available.
---

# impl

実装計画を立て、実装を進める。

## 原則

- 実装前に前提を細かく確認
- メインエージェントは監督、人間窓口、厳しい確認を担当
- 細かい作業、雑務、簡単な実装は subagent へ任せる
- 通常の実装も、適切な subagent が使えるなら任せる
- メインエージェントは差分を必ず読む
- 未確認のまま完了扱いしない

## 手順

1. 前提確認
   - `AGENTS.md`
   - 関連 `INDEX.md`
   - 要件文書
   - 設計判断
   - 現在の差分

2. 実装計画
   - 目的
   - 変更対象
   - 作業単位
   - 検証方法
   - 危険
   - 人間判断が必要な点

3. 分担
   - subagent が使える場合、独立した小作業を渡す
   - subagent へ渡す内容は目的、対象、制約、完了条件だけにする
   - メインエージェントは統合前に必ず差分確認

4. 実装
   - 既存の型、命名、構造へ合わせる
   - 余計な修正を混ぜない
   - 関係ない変更は戻さない
   - 必要なら広く直す

5. 確認
   - 差分確認
   - 試験または代替確認
   - 未確認事項の明記

## 完了

- 実装内容
- 確認結果
- 未確認事項
- 次の推奨 skill 1-3 件
  - `verify`: 実行確認を深める
  - `review`: 品質確認へ進む
  - `docs`: 文書更新が必要
