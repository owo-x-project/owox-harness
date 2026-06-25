# skill script 化誘導

## 状態

草案。次回以降に詳細を詰める。

## 背景

スキルは散文だけでは検証しにくい。
一方、繰り返す手順の中には機械処理へ落とせる核がある。
機械化できる部分は script に寄せ、テストを書ける形へ育てたい。

## 目的

- 繰り返す手順を script 型 skill へ育てやすくする
- テスト可能な処理を散文に残さない
- 既存の skill 登録・昇格ゲートを壊さない
- 自動雛形生成はせず、人間判断を残す

## 対象

- routine 提案
- skill 正本
- `scripts/`
- `tests/`
- `skill.register`
- `skill.promote`

## 対象外

- skill 雛形の自動生成
- LLM 評価を登録ゲートにすること
- 人間承認の自動化
- 外部サービス操作の script 化

## 仕様

routine 提案を2種類に分ける。

```text
kind = "skill"
kind = "script-skill"
```

`script-skill` 候補にする必須条件:

- 同じ手順が3回以上出た
- 入力と出力がだいたい決まる
- 失敗を終了コードで表せる
- リポジトリ内で閉じる
- 秘密値を扱わない
- 小さい検査用データで試せる

必須条件を1つでも欠く場合は、通常の skill 候補へ留める。

強い判定条件:

- 同じコマンド列を繰り返す
- `rg` / `sed` / `awk` / `jq` / `yq` などで走査・抽出・変換する
- `cargo test` / `npm test` / `pytest` など検査実行が核
- ファイル一覧を集めて分類する
- 正本や設定の形式を検査する
- 差分や参照を集計する
- 同じ置換・整形・生成を繰り返す

必須条件をすべて満たし、強い判定条件が1つ以上あれば `script-skill` とする。

除外条件:

- 設計判断
- レビュー判断
- 人間承認
- 文章の意味判断
- 破壊的操作
- 外部サービス操作
- 秘密値や認証情報を扱う
- 一度きりの作業
- AI の自由なコード編集が必要な作業

除外条件に当たる場合は通常 skill 候補へ降格する。

## 利用履歴

利用履歴に生コマンドは残さない。
script 判定のため、保存する場合も安全な分類だけにする。

例:

```text
Bash:rg
Bash:sed
Bash:cargo-test
Read
Edit
```

引数・パス・検索語は残さない。

## 出力

`verify.run` の `data.routine_suggestions` と `next` に出す。

例:

```text
kind: script-skill
confidence: high
reason:
- repeated 4 times
- command core is rg + sed
- exit code can express failure
suggested_script: scripts/check-refs.sh
test_hint: tests/check-refs.sh with fixture files
```

初期値:

- 繰り返し回数: 3
- 候補表示: 上位3件
- 判定: 必須条件すべて + 強い判定条件1つ以上

## 登録ゲート

既存の skill 契約を使う。

- `scripts/<name>` を SKILL.md が参照するなら実在必須
- tests は実行ビット必須
- `implicit=true` は tests 必須
- `skill.register` がテストを走らせる

追加助言:

- script があるのに tests が無い場合、明示 skill なら登録可だが warning
- script があるのに SKILL.md で使い方を書いていない場合、draft
- tests が script を呼んでいない場合、warning 候補

## 検証

- 同じ手順3回以上で routine 候補が出る
- 必須条件を満たす機械処理が `script-skill` になる
- 除外条件に当たるものは通常 skill 候補へ降格する
- usage に引数・パス・検索語が残らない
- `verify.run` が `kind` / `reason` / `suggested_script` / `test_hint` を返せる
- 既存の `skill.register` / `skill.promote` が壊れない

## 未決事項

- 安全な分類の語彙
- `confidence` の算出
- warning をどこへ出すか
- tests が script を呼ぶかをどこまで機械検査するか
- 明示 skill に tests 無しをどこまで許すか
