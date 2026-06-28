# tool 整理

## 状態

草案。次回以降に詳細を詰める。

## 背景

MCP tool 数は増えている。tool 自体は実体なので細かく分かれていてもよいが、AI が選ぶ面で迷うと導線が弱くなる。
物理削除より先に、入口 skill と `SessionStart` での見せ方を整理する。

tool をどれをいつ使うかは行動軸（`docs/decisions/20260627-判断2軸と対話kickoff.md`）。owox が断定的に示し、AI は人間へ tool 選択を打診しない。本整理はその断定導線を整える作業であり、人間 gate は製品意図正本・安全境界・不可逆操作のみに掛ける。

## 目的

- AI が tool 選択で迷わない
- `SessionStart` へ個別 tool 説明を置かない
- 入口 skill が tool 実行順を決める
- tool description を短く保つ
- 危険度の違う操作を無理に統合しない

## 対象

- MCP tool
- 入口 skill
- `SessionStart`
- tool description

## 対象外

- 初期実装での tool 物理削除
- 危険度の違う tool の mode 統合
- kickoff 詳細化

## 仕様

tool をカテゴリで扱う。

1. 読む
   - `context`
   - `next`
   - `rules.lookup`
   - `glossary.lookup`
   - `practice.lookup`
   - `knowledge.*`
   - `profile.get`
   - `branch.notes`

2. 記録・判断
   - `decision.record`
   - `gate.*`
   - `correction.note`

3. 要件・作業
   - `requirement.*`
   - `task.*`
   - `branch.note`

4. 検査・レビュー
   - `verify.run`
   - `review.lenses`
   - `release.check`

5. 正本変更
   - `canon.add`
   - `canon.propose`
   - `canon.detect`

6. 性質・状態
   - `state.set`
   - `profile.set`
   - `profile.detect`

7. skill・経験
   - `skill.*`
   - `experience.*`

## 入口 skill

- `$next`: `next` → 必要なら `context scope="diff"`。`next` は人間 gate（製品意図）と owox 断定の次行動を分けて返す
- `$status`: `next` → `verify.run` → gate 要約。次行動は断定形で示す
- `$review`: `review.lenses` → `verify.run` → `context scope="diff"`
- `$verify`: `verify.run`
- `$skill`: `skill.list` → routine / script-skill → `skill.register` / `skill.promote`
- `$decide`: `decision.record` / `gate.approve`
- `$task`: `task.*`
- `$req`: `requirement.*`
- `$kickoff`: 最後に全体導入

## 統合候補

今すぐ統合しない。将来候補として扱う。

- `knowledge.list/get/lookup` → `knowledge` + mode
- `branch.note/notes` → `branch` + mode
- `profile.get/set/detect` → `profile` + mode
- `gate.auto_*` → 自動承認系の整理候補
- `task.create/list/update/note/link/close/drop` → 当面維持

## 残すべき分割

- `canon.add` と `canon.propose`
  - AI 直接追加と人間承認変更で性質が違う

- `verify.run` と `review.lenses`
  - 機械検査とレビュー観点選択で性質が違う

- `rules.lookup` と `glossary.lookup`
  - 返る形が違う

- `requirement.*`
  - 要件操作は明示性が大事。mode 統合すると事故りやすい

## 統合判断基準

統合する条件:

- 返り値の形がほぼ同じ
- 操作の危険度が同じ
- AI が迷うだけで、人間の安全判断に影響しない
- mode にしても説明が短くなる

統合しない条件:

- 人間承認の有無が違う
- 失敗時の扱いが違う
- 返り値が大きく違う
- 操作の意味が違う
- 事故時の影響が違う

## 実装順

1. `SessionStart` から個別 tool 説明を消す
2. 入口 skill 本文を tool 実行順へ寄せる
3. tool description を短くする
4. `context scope="diff"` を足す
5. mode 統合は後回し

## tool description 基準

- 1文だけ
- 選択に必要な役割だけ書く
- 安全条件がある tool だけ安全条件を書く
- 例・細かい引数説明・後続導線は書かない

## 検証

- `SessionStart` が個別 tool の詳細説明を持たない
- 入口 skill が必要な tool 実行順を示す
- tool description が選択に必要な情報だけを持つ
- tool 数を増やさずに導線が改善する
- 危険度の違う操作を統合していない

## 未決事項

- 自動承認系 tool の整理方法
- profile / knowledge / branch を mode 統合する時期
- 入口 skill 本文の最終文面
