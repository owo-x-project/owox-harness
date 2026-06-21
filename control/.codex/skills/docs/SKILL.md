---
name: docs
description: Use when updating README.md, AGENTS.md, directory INDEX.md files, requirement documents, or checking consistency between documentation and implementation.
---

# docs

文書を更新し、実装とのずれを防ぐ。

## 文書の役割

- `README.md`: 人間向け
- `AGENTS.md`: control harness の最上位正本
- `INDEX.md`: AI 向けディレクトリ・ファイル説明の正本
- `docs/requirements`: 要件正本
- `docs/handoff`: 引き継ぎ文書、git 管理対象外

## 原則

- 自明な説明を増やさない
- 実装と文書のずれを残さない
- 最上位 `INDEX.md` は作らない
- サブディレクトリの `INDEX.md` は必要な場合だけ作る
- 人間向けと AI 向けを混ぜない

## 手順

1. 対象確認
   - 変更内容
   - 読者
   - 永続化する価値

2. 更新判断
   - 人間が読むなら `README.md`
   - エージェントが読むなら該当ディレクトリの `INDEX.md`
   - 行動規範なら `AGENTS.md`
   - 要件なら `docs/requirements`
   - 引き継ぎなら `docs/handoff`

3. 更新
   - 短く書く
   - 役割を書く
   - 古い説明を消す
   - 参照はパスで書く

4. 整合確認
   - 実装と合うか
   - 他文書と矛盾しないか
   - 不要な重複がないか

## 完了

- 更新文書
- 残る文書差分
- 次の推奨 skill 1-3 件
  - `review`: 文書の妥当性確認が必要
  - `commit`: 文書変更を保存する
  - `plan`: 要件化が必要
