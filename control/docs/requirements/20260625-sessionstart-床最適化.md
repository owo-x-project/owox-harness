# SessionStart 床最適化

## 状態

草案。次回以降に詳細を詰める。

## 背景

`SessionStart` は常に読む床。ここが厚いと、重要な案内ほど流れる。
現在は向き付け、詳細な意図ルーティング、orchestration、用語、practices、skills などが混在している。

床は薄い地図にし、詳しい導線は skill へ寄せ、実動作は tool へ寄せる。

## 目的

- `SessionStart` の常時文脈を軽くする
- skill を導線、tool を実体として分離する
- kickoff は最後に、薄い床と整理済み導線を束ねる入口にする
- tool 詳細説明と床文脈の重複を減らす

## 対象

- `floor_context`
- `SessionStart`
- 入口 skill
- tool description

## 対象外

- hook 体系の作り直し
- tool の新設
- kickoff の詳細化
- 正本の直読み許可

## 仕様

`SessionStart` に残すもの:

1. Vision
   - プロジェクトの目的だけ
   - 長い価値観・原則は出さない

2. Response language
   - 人間への応答言語

3. Phase
   - `initial / stable / maintenance`
   - owox 標準の短い1文 + 現在 phase rules だけ

4. Direct canon rule
   - `.owox` の正本を直接読まない・直接編集しない
   - 変更は `canon.add` / `canon.propose`

5. Entry map
   - 詳細ではなく入口だけ

```text
Use kickoff to orient.
Use next to choose work.
Use context to find what to read.
Use verify before finishing.
Use review to inspect changes.
Use skill to grow or manage skills.
Use rules.lookup, glossary.lookup, practice.lookup when terms or rules matter.
```

6. Current pressure
   - 1行だけ
   - open decisions / ready tasks / stale items などの有無
   - 詳細は `next`

`SessionStart` から落とすもの:

- 詳細な意図ルーティング
- orchestration 長文
- role 一覧
- profile 4軸詳細
- glossary terms 一覧
- practices 本文
- skills 一覧
- read next の細かい説明
- rules / brand 本文
- tool の詳細な使い分け

## skill 側の強化

- `$review`: `review.lenses` → `verify.run` → `context scope="diff"`
- `$status`: `next` + 必要なら `verify.run`
- `$skill`: `skill.list` + routine / script-skill の扱い
- `$verify`: `verify.run`
- `$next`: `next` + 必要なら `context scope="diff"`
- `$kickoff`: 最後に薄い床を補う厚い導入

## 実現方針

- `floor_context` を小さくする
- `render_orchestration` は床から外す
- `render_skills_section` は床から外す
- glossary / practices は一覧を出さず lookup へ寄せる
- 詳細な行動手順は入口 skill へ寄せる
- tool description は短くし、選択に必要な情報だけ残す

## 検証

- `SessionStart` の文字量が減る
- Vision / phase / response language / canon 直読み禁止 / entry map は残る
- glossary / practices / skills の一覧が床に出ない
- `$review` が `context scope="diff"` まで導線を持つ
- `$skill` が script-skill 導線を持つ
- kickoff を使わなくても最低限迷わない

## 未決事項

- Current pressure の算出範囲
- `context` に移す orchestration 情報の扱い
- tool description をどこまで短くするか
- skill 本文の最終文面
