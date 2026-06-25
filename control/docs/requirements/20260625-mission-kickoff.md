# session 任務種別と kickoff

## 状態

草案。次回以降に詳細を詰める。

## 背景

`kickoff` は作業開始時に人間を質問攻めにし、重要判断を徹底的に決める入口。
ただし AI の自律判断は信用しすぎない。

一括診断や自動書込ではなく、session 中の tool 全体の動作を `kickoff` 向けへ寄せる任務種別として扱う。

## 目的

- `kickoff` 開始後、未確定事項を 1 つずつ詰める
- 人間が握る判断と AI が仮決定してよい細部を分ける
- `next` / `context` / `verify.run` などを任務へ最適化する
- tool を増やしすぎず、共通の任務切替で実現する

## 対象

- session 任務状態
- `$kickoff` skill
- `mission.start`
- `mission.finish`
- `mission.cancel`
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

### 起動

`kickoff` は完全手動。
`$kickoff` が `mission.start type="kickoff"` を呼ぶ。

`SessionStart` や発話検知では開始しない。

### session 任務

session は任務種別を 1 つ持てる。

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

### 終了

- `mission.finish`: 任務完了
- `mission.cancel`: 任務中止

終了しない限り、任務適応は続く。

### kickoff 中の挙動

`next`:

- 次に決める未確定事項を 1 件返す
- 推奨案と理由を返す
- 決定者を `human` / `ai` / `defer` で返す
- 全質問に AI の推奨案と理由を必ず添える
- 人間が迷わないように、選択肢だけを出して終わらない

`context`:

- 判断材料を返す
- 既存コードから分かることを返す
- 未確定根拠を返す
- 本文や大きな raw data は返さない

`verify.run`:

- 未決の残りを検査する
- AI 仮決定の残りを検査する
- 正本化前の抜けを検査する

`profile.detect`:

- 未設定なら積極的に候補を出す
- 根拠つき draft を返す
- 自動確定しない

`canon.detect`:

- 正本が薄い領域を探す
- 質問候補にする
- 自動追加しない

### kickoff の質問形

毎回 1 論点だけ返す。
全ての質問は推奨案と理由を必須にする。

```text
決めること: AI に任せてよい範囲
推奨: supervised
理由: 安全境界がまだ薄い
決定者: human
選択肢:
- supervised
- guarded
- free
次: 回答待ち
```

AI が仮決定してよい場合:

```text
決めること: 初期 task 分割
推奨: AI仮決定
理由: コード構造から機械的に分けられる
決定者: ai
次: AI が仮決定して提示
```

### kickoff の段階

固定段階 + 動的枝で進める。
大枠は固定し、既に決まっている項目は聞かず、回答やコード状況に応じて枝分かれする。

1. 入口確認
   - session が `kickoff` 任務であること
   - 既存 `target harness` の有無
   - 既に決まっている項目

2. 目的
   - repo で達成したいこと
   - 誰のための repo か
   - 成功条件

3. 人間が握る判断
   - AI に任せない判断
   - AI が仮決定してよい細部
   - 判断材料不足時の扱い

4. 安全境界
   - 止める操作
   - 人間確認が必要な変更
   - 秘密情報
   - 外部通信
   - 削除
   - 生成物上書き

5. 作業の型
   - 通常作業の進め方
   - review 重視か実装速度重視か
   - 検証をどこまで必須にするか

6. `target harness` 初期構成
   - `AGENTS.md`
   - skills
   - hooks
   - tools
   - MCP
   - rules
   - 足りないもの
   - 薄いもの
   - 邪魔なもの

7. rules / practices / glossary
   - 最初から固定する rule
   - 成長させる practice
   - 登録すべき用語
   - 常時読むもの
   - 必要時だけ読むもの

8. skill 化
   - よくある作業
   - script 化できる作業
   - 人間に聞くべき作業
   - AI に任せる作業

9. 初期 task
   - 最初にやる 1 件
   - 後回しにするもの
   - 完了条件
   - 検証方法

10. 終了判定
   - 確定事項
   - AI 仮決定
   - 未決
   - 正本化候補
   - `mission.finish` 可否

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
- コードから分かる項目は AI が推定し、重要なら人間確認に回す
- 重要判断は必ず人間へ質問する
- 細部は AI 仮決定として提示できる
- 回答が曖昧なら、次ターンで 1 問だけ追加質問する
- 判断材料不足なら `context` で調べてから質問する

### 正本化

kickoff 中は原則書かない。

人間確認後だけ既存 tool へ分配する。

- 性質確定: `profile.set`
- 正本追加: `canon.add` / `canon.propose`
- 作業追加: `task.create`
- 判断記録: `decision.record`

## 実現方針

- `owox.kickoff` 専用 tool ではなく `mission.start` を使う
- `$kickoff` skill は `mission.start type="kickoff"` と `next` を呼ぶ入口にする
- 各 tool は session 任務を読んで返却内容を変える
- 任務種別は少数に抑える

## 検証

- `$kickoff` で `mission: kickoff` になる
- kickoff 中の `next` が通常作業ではなく未確定事項を返す
- kickoff 中の質問が必ず推奨案と理由を含む
- kickoff 中の `context` が判断材料を返す
- kickoff 中の `verify.run` が未決と AI 仮決定を検査する
- 任務状態が tool 出力に必ず出る
- `mission.finish` / `mission.cancel` で通常挙動へ戻る
- kickoff 中に自動正本書込しない

## 未決事項

- 任務状態の保存場所
- session をまたぐ任務継続の可否
- `work` 任務を明示状態にするか、任務なしを通常扱いにするか
- `mission.start` の返却形式
- `mission.finish` 時の正本化候補のまとめ方
