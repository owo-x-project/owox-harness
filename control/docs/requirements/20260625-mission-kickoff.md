# session 任務種別と kickoff

## 状態

改訂。判断 2 軸（`docs/decisions/20260627-判断2軸と対話kickoff.md`）に沿って対話型へ書き直し。

## 背景

`kickoff` は作業開始時に、人間が握る製品意図を引き出し切る入口。
ただし AI の自律判断は信用しすぎない。

一括診断や自動書込ではなく、session 中の tool 全体の動作を `kickoff` 向けへ寄せる任務種別として扱う。

2026-06-27 の対話検証（`docs/validation/20260627-要件群対話検証.md`）で、旧設計が機械的で、AI が考えを押し付け、行動軸まで人間へ質問する粗が出た。判断 2 軸に沿って是正する。

- 製品意図軸（人間が握る）: 何を / 誰のため / 成功条件 / リスク許容 / 止める操作 / 優先順位。kickoff はこれを引き出す
- 行動軸（owox が握る）: tool 選択 / verify 可否 / skill 化 / profile / rules 下地 / 索引 / 庭師。kickoff は owox が決めて人間へ確認する

## 目的

- `kickoff` 開始後、製品意図の未確定を対話で詰める
- 人間が握る製品意図と、owox が決める行動軸を分ける
- 質問を引き出し / 確認 / 判断の 3 種に分け、押し付けを防ぐ
- `next` / `context` / `verify.run` などを任務へ最適化する
- tool を増やしすぎず、共通の任務切替で実現する

## 対象

- session 任務状態
- `$kickoff` skill
- `mission.start`
- `next`
- `context`
- `verify.run`
- `profile.detect`
- `canon.detect`

## 対象外

- kickoff の自動開始
- kickoff 中の自動正本書込
- 要件仕様の深掘り全般
- 任務種別の大量追加

## 仕様

### 切替

`kickoff` は完全手動。
`$kickoff` が `mission.start type="kickoff"` を呼ぶ。

`SessionStart` や発話検知では切替しない。

`mission.start` の返却:

- `mission`
- `next_preview`
- kickoff 切替時だけ `data.kickoff`
  - `unresolved`
  - `ai_drafts`
  - `human_decisions`
  - `next_question`
  - `ready_to_return`
  - `canonicalization_candidates`

### session 任務

session は任務種別を 1 つ持てる。
未切替時も `work` を持つ。

初期候補:

- `work`: 通常作業
- `kickoff`: 初期判断詰め
- `review`: 差分確認
- `verify`: 完了前検査
- `handoff`: 引き継ぎ

任務中の tool 出力には必ず現在任務を含める。

```text
mission: kickoff
```

隠れ状態にしない。

### 通常作業へ戻る

`mission.start type="work"` で通常作業へ戻る。

切替しない限り、任務適応は続く。

### kickoff 以外の任務の挙動

`docs/decisions/20260628-任務別行動軸.md`。mission は行動軸 (owox が断定する今すぐやること) と
verify/context の焦点を切り替える。新しいゲートは作らない (判断2軸より)。

- `review`: `next` の先頭行動を「差分を読む (context scope diff) → 所見を decision.record / task.create で記録。新規実装はしない」に。`verify.run` の次の手は所見化へ向ける
- `verify`: 先頭行動を「verify.run → 未トレース要件を検査へ link → 作業範囲確認」に。検査結果自体は mission 不問で一貫
- `handoff`: 先頭行動を「検証状態 + 未決を要約 → 残作業を task 化 → handoff skill で引き継ぎ文」に
- 非 `work` 任務は保留が無くても沈黙せず、任務の作業を断定で出す

### kickoff 中の挙動

`next`:

- 次に詰める製品意図を 1 件返す
- 質問種別を `引き出し` / `確認` / `判断` で返す
- `判断` のときだけ推奨案と理由と選択肢を返す
- `確認` のときは owox が決めた構成・理解を提示し、否認 / 修正だけ求める
- `引き出し` のときは開いた質問を返し、推奨は貼らない
- 同時に、AI が今すぐやる行動（行動軸）を断定で返す
- 選択肢だけを出して終わらない

`context`:

- 判断材料を返す
- 既存コードから分かることを返す
- 未確定根拠を返す
- 本文や大きな raw data は返さない

`verify.run`:

- 未決の残りを検査する
- AI 仮決定の残りを検査する
- 正本化前の抜けを検査する
- kickoff 中は `data.kickoff` に戻る前まとめを返す
  - `ready_to_return`
  - `canonicalization_candidates`

`profile.detect`:

- 未設定なら積極的に候補を出す
- 根拠つき draft を返す
- 自動確定しない

`canon.detect`:

- 正本が薄い領域を探す
- 質問候補にする
- 自動追加しない

### kickoff の質問形

毎回 1 論点だけ返す。質問種別で形を変える。

`引き出し`（製品意図を開いて聞く。推奨を貼らない）:

```text
種別: 引き出し
聞くこと: この repo で何を達成したい? 誰のため? 成功条件は?
次: 回答待ち
```

`確認`（owox が決めた行動軸・理解を提示。否認 / 修正だけ）:

```text
種別: 確認
決めたこと: project nature = script
根拠: package.json と src 構成から機械的に判定
次: 違えば修正を、よければ承認を
```

`判断`（人間しか決められない意図 / 安全の分岐。推奨+理由+選択肢）:

```text
種別: 判断
決めること: AI に任せてよい範囲
推奨: supervised
理由: 安全境界がまだ薄い
選択肢:
- supervised
- guarded
- free
次: 回答待ち
```

### kickoff の段階

大枠は固定し、既に決まっている項目は聞かず、回答やコード状況に応じて枝分かれする。
各段階は軸で扱いが変わる。製品意図軸は引き出し対話で詰める。行動軸は owox が決めて確認に回す。

1. 入口確認（確認）
   - session が `kickoff` 任務であること
   - 既存 `target harness` の有無
   - 既に決まっている項目

2. 目的（引き出し・製品意図軸）
   - repo で達成したいこと
   - 誰のための repo か
   - 成功条件

3. リスク許容（判断・製品意図軸）
   - AI に任せてよい範囲
   - 判断材料不足時の扱い

4. 安全境界（判断・製品意図軸）
   - 止める操作
   - 人間確認が必要な変更
   - 秘密情報
   - 外部通信
   - 削除
   - 生成物上書き

5. 作業の型（製品意図軸は判断・行動軸は確認）
   - review 重視か実装速度重視か（判断）
   - 通常作業の進め方 / 検証の必須範囲（owox 決定 → 確認）

6. `target harness` 初期構成（確認・行動軸）
   - owox が現状を診断し構成を決める（`AGENTS.md` / skills / hooks / tools / MCP / rules）
   - 足りない / 薄い / 邪魔なものを owox が判定
   - 人間へは決定結果を提示し、否認 / 修正だけ求める

7. rules / practices / glossary（確認・行動軸）
   - owox が固定 rule / 成長 practice / 登録用語 / 読取タイミングを決める
   - 製品意図に直結する rule（安全・ブランド）だけ判断として人間へ回す

8. skill 化（確認・行動軸）
   - owox が常作業 / script 化可否 / AI 委任範囲を決める
   - 人間へは結果を提示し確認を求める

9. 初期 task（行動軸は確認・優先度は判断）
   - 最初にやる 1 件 / 後回し / 完了条件 / 検証方法は owox 決定 → 確認
   - 優先順位は製品意図軸なので判断として人間へ

10. 終了判定（確認）
   - 確定した製品意図
   - owox が決めた行動軸
   - 未決
   - 正本化候補
   - `mission.start type="work"` へ戻してよいか

### 次の質問の優先順位

`kickoff` 中の `next` は次の順で質問を選ぶ。

1. 後続を止める未決
2. 人間しか決められない未決
3. 安全に関わる未決
4. 正本構成に関わる未決
5. AI が仮決定できる細部
6. 初期 task

### 動的枝

- 既に決まっている項目は聞かず確認だけにする
- コードから分かる行動軸は owox が決め、確認に回す
- 製品意図軸の重要判断は必ず人間へ質問する
- 引き出しの回答が曖昧なら、次ターンで 1 問だけ鋭い追撃をする
- 判断材料不足なら `context` で調べてから質問する
- 人間は自然発話でよい。AI が owox を代理操作する

### 正本化

製品意図正本（requirements / decisions）は人間確認後だけ書く。
行動軸の生成物（profile / 索引 / setup 書込など）は owox が決めて書いてよい。

確定後、既存 tool へ分配する。

- 性質確定: `profile.set`（行動軸・確認後）
- 正本追加: `canon.add` / `canon.propose`（意図軸・人間確認後）
- 作業追加: `task.create`
- 判断記録: `decision.record`（意図軸・人間確認後）

## 実現方針

- `owox.kickoff` 専用 tool ではなく `mission.start` を使う
- `$kickoff` skill は `mission.start type="kickoff"` と `next` を呼ぶ入口にする
- 任務は常に 1 つあり、既定は `work`
- 各 tool は session 任務を読んで返却内容を変える
- 任務種別は少数に抑える

## 検証

- `$kickoff` で `mission: kickoff` になる
- kickoff 中の `next` が通常作業ではなく未確定の製品意図を返す
- `next` が質問種別（引き出し / 確認 / 判断）を返す
- `判断` のときだけ推奨案と理由が付き、`引き出し` には付かない
- `next` が同時に AI の今すぐやる行動（行動軸）を断定で返す
- harness 構成（段階 6/7/8）が人間質問でなく owox 決定 → 確認になる
- kickoff 中の `context` が判断材料を返す
- kickoff 中の `verify.run` が未決と AI 仮決定を検査する
- 任務状態が tool 出力に必ず出る
- `mission.start type="work"` で通常挙動へ戻る
- 製品意図正本は人間確認後だけ書く。行動軸生成物は owox が書いてよい

## 未決事項

- 任務状態の保存場所
- session をまたぐ任務継続の可否
- `work` 任務を明示状態にするか、任務なしを通常扱いにするか
