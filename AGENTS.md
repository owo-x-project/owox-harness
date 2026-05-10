## 最初に読むもの

1. `.agents/project.md`
2. 対象の `.agents/tasks/task-*.md`
3. 必要に応じて `docs/project/index.md`

## 責務の境界

- `.agents/`: AI 専用の制約、作業手順、skill、task 文脈キャッシュ
- `docs/project/`: 人間向けのプロジェクト正本

## 作業ルール

- セッション開始時、ユーザー言語が日本語なら `util-genshijin`、英語なら `util-caveman` を必ず読み込む
- まず `.agents/project.md` を読む
- 対象 task がある場合は `.agents/tasks/task-*.md` を読む
- `docs/project/index.md` から必要なカテゴリ `index.md` と個票へ降りる
- 必読用語は `docs/project/glossary/core.md` を優先する
- archive は明示的に必要な場合だけ読む
- 不要な全文走査と重複読書を避ける
- 正本が不足している場合は決め打ちで進めず、必要な更新 task に戻す
- `request_user_input` ツールが使えない場合は通常メッセージで確認し、確認結果を task に転記する

## コマンド実行ルール

- あらゆるコマンド実行時は、コマンドの前に `rtk` を付ける
- 例: `git status` ではなく `rtk git status` を実行する
