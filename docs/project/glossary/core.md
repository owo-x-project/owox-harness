# Core Glossary

このファイルは、プロジェクト全体で必読となる共通用語を置きます。

## Plan

実装計画やロードマップの正本。`docs/project/plan/` に置き、検証可能な phase と実作業単位の plan task に分解する。

## Plan Task

`docs/project/plan/<plan-slug>/phase-*/task-*.md` に置く、実装計画上の作業単位。1 Markdown ファイルにつき 1 task とし、前提条件、作業内容、完了条件、検証方法、依存関係、成果物を記載する。

## 作業ログ Task

`.agents/tasks/task-*.md` に置く、AI エージェントの作業ログまたは文脈キャッシュ。plan task の正本ではなく、作業中の判断、検証結果、未完了事項を引き継ぐために使う。
