---
name: target-validate
description: Use when validating owox-harness outputs in a target repo, checking generated target harness files, tools, MCP, CLI behavior, and cross-CLI usability.
---

# target-validate

target repo で target harness を検証する。

## 目的

`owox-harness` が生成・管理する target harness が、実際の開発で使えるか確認する。

## 原則

- control repo と target repo を混ぜない
- 生成元と生成物を分けて見る
- target harness の各 CLI 向け差分を確認する
- 手動確認だけで終えない
- target repo の汚れを最後に確認する

## 手順

1. 前提確認
   - 検証する `owox-harness` の版
   - target repo の場所
   - 対象 CLI
   - 期待する target harness

2. 生成確認
   - 生成命令
   - 生成ファイル
   - 上書き挙動
   - 再実行時の差分

3. 動作確認
   - CLI ごとの読込
   - skill
   - hook
   - subagent
   - tool
   - MCP

4. 清潔さ確認
   - 不要ファイル
   - git 管理対象
   - target repo の未意図差分
   - `scripts/check-target-cleanliness.sh` が使える場合は使う

5. 結果整理
   - 成功
   - 失敗
   - 再現手順
   - 修正案

## 完了

- 検証対象
- 結果
- 失敗と再現手順
- 未確認事項
- 次の推奨 skill 1-3 件
  - `impl`: 失敗修正が必要
  - `review`: target harness の設計確認が必要
  - `docs`: 検証手順を文書化する
