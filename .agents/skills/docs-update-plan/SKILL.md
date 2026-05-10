---
name: docs-update-plan
description: 実装計画やロードマップを検証可能なフェーズと task に分解して追加、更新、正規化するときに使用する
argument-hint: "計画=<計画名> 目的=<達成したい状態> 範囲=<対象範囲>"
---

## 目的

実装計画やロードマップを、検証可能なフェーズと AI エージェントが実行しやすい task に分解して `docs/project/plan/` に記録する。

## 前提資料

- `docs/project/index.md`
- `docs/project/glossary/core.md`
- `docs/project/plan/index.md`
- `.agents/skills/_shared/document-reference-rules.md`
- `.agents/skills/_shared/document-update-checklist.md`
- `.agents/skills/docs-update-plan/references/plan.template.md`
- `.agents/skills/docs-update-plan/references/phase.template.md`
- `.agents/skills/docs-update-plan/references/task.template.md`
- `.agents/skills/docs-update-plan/references/best-practices.md`
- 関連 requirement / spec / ADR / validation / pattern / proposal / research

## やること

1. 必要なら `request_user_input` で目的、対象範囲、非スコープ、前提、期限、優先順位、検証可能な成果を確認する
2. 既存の `docs/project/plan/` を確認し、重複や置換対象がないか調べる
3. plan 全体の目的、スコープ、非スコープ、完了定義、関連正本を整理する
4. plan を検証可能な phase に分け、各 phase の目的、前提、完了条件、検証方法を明確にする
5. 各 phase を実作業単位の task に分け、1 task ごとに 1 Markdown ファイルを作成または更新する
6. `docs/project/plan/<plan-slug>/index.md`、各 `phase-*/index.md`、各 `task-*.md` を作成または更新する
7. `.agents/skills/_shared/document-reference-rules.md` に従い、対応する `index.md` を必ず更新する
8. requirement / spec / ADR / validation / pattern / proposal / research への昇格や参照の要否を確認する

## ルール

- plan は実装計画の正本として扱う
- plan は検証可能な phase に分ける
- phase ごとにサブディレクトリを切る
- phase ごとに実際に作業する単位を task として分割する
- 1 task = 1 Markdown ファイルを必ず守る
- task は前提条件、完了条件、検証方法、作業内容を省略しない
- task は AI エージェントが 1 セッションで高品質に完了しやすい作業量にする
- plan task と `.agents/tasks/task-*.md` の作業ログ task を混同しない
- plan task に実施ログ、試行錯誤、会話の文脈キャッシュを書かない
- `.agents/tasks/task-*.md` に plan の正本を置かない

## 確認事項

- plan 全体の目的、範囲、非スコープ、完了定義が明確である
- phase が検証可能な単位に分かれている
- 各 phase に `index.md` がある
- 各 task が 1 Markdown ファイルに分かれている
- 各 task に前提条件、作業内容、完了条件、検証方法、依存関係、成果物がある
- `docs/project/plan/index.md` と各階層の `index.md` を更新した
- plan task と `.agents/tasks/` の作業ログ task の責務が混在していない
