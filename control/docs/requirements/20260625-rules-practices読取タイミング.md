# rules practices 読取タイミング

## 状態

草案。次回以降に詳細を詰める。

## 背景

rules / practices は必要な時に届くべきだが、ユーザー発話の語彙から intent を判定すると壊れる。
i18n はしないため、日本語/英語/表現ゆれを語彙登録で吸収する設計は避ける。

読むタイミングは、機械的に分かる事実へ寄せる。

## 判断 2 軸での位置づけ

`docs/decisions/20260627-判断2軸と対話kickoff.md` に従う。

- いつ rules / practices を読むか・どの配送面で出すか・操作種類の検知・索引は行動軸。owox が断定して出す。AI は人間へ打診しない
- rules の内容 (安全・依存・削除・正本変更など) と、その trigger を変える操作は製品意図軸。人間 gate を残す (`canon.propose` / `canon.add`)

## 目的

- rules / practices を必要な時だけ出す
- 語彙ベースの intent 判定を避ける
- i18n 不要で安定する
- `SessionStart` の常時注入を減らす

## 対象

- rules
- practices
- `SessionStart`
- `PreToolUse`
- `context scope="diff"`
- `verify.run`
- `review.lenses`

## 対象外

- ユーザー発話の語彙による intent 判定
- i18n
- 新規 lookup tool
- rules / practices の常時全量注入

## 仕様

trigger は3つに絞る。

```text
trigger = "always" | "path" | "operation"
```

- `always`
  - 常時出す
  - 上限あり

- `path`
  - 対象ファイルで出す
  - glob で判定する

- `operation`
  - tool / 操作種類で出す
  - ユーザー発話ではなく、実際の tool 入力から判定する

operation 候補:

```text
read
edit
delete
commit
review
verify
canon-change
dependency-change
requirement-change
skill-change
```

検知元:

- `Read` / `cat` / `sed -n` / `head` / `tail` → `read`
- `apply_patch` / `Edit` / `Write` → `edit`
- `rm` / delete patch → `delete`
- `git commit` → `commit`
- `review.lenses` → `review`
- `verify.run` → `verify`
- `canon.add` / `canon.propose` → `canon-change`
- `Cargo.toml` / `package.json` などの変更 → `dependency-change`
- `.owox/requirements/**` / requirement tool → `requirement-change`
- `.owox/skills/**` / skill tool → `skill-change`

## 書式案

既存の箇条書きに属性行を足す。

```md
- 削除前に参照検索と verify を通す
trigger: operation
operation: delete
path: src/**
```

複数指定も許す。

```md
operation: edit, delete
path: crates/core/**
```

属性なし既定:

- rules: `operation`
- practices: `path`

ただし既定の詳細は実装時に決める。

## 配送

- `SessionStart`
  - `always` だけ

- `PreToolUse`
  - `operation` + `path`

- `context scope="diff"`
  - `path`

- `verify.run`
  - `operation=verify`

- `review.lenses`
  - `operation=review`

- `rules.lookup` / `practice.lookup`
  - 手動取得

## rules と practices の違い

rules:

- 固定層
- 人間が握る
- 安全・依存・削除・正本変更など
- trigger 変更は `canon.propose`

practices:

- 成長層
- AI が育てる
- 作業のコツ・繰り返し改善
- 追加は `canon.add`

## 検証

- ユーザー発話の語彙に依存しない
- `operation=delete` が削除操作前に出る
- `path` が対象ファイルに応じて出る
- `SessionStart` には `always` だけ出る
- `context scope="diff"` が差分に合う path rules / practices を返せる
- 読取タイミング・配送・検知は owox が断定して出し、AI が人間へ打診しない (行動軸)
- rules の内容と trigger 変更は人間 gate へ寄る (製品意図軸)

## 未決事項

行動軸 (owox が実装時に断定):

- operation の最終語彙
- path glob の書き方
- always 上限
- 属性なし既定

製品意図軸 (人間 gate に関わる):

- rules の trigger 変更をどこまで機械強制するか
