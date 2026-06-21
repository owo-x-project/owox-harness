# SuperClaude Framework

## 調査日

2026-06-12

## 分類

作業流れ、target harness 参考。

Claude Code 向けの命令、agent、モード、MCP 連携をまとめる。

## 概要

SuperClaude Framework は、Claude Code の使い方を拡張する仕組み。

命令、agent、モード、MCP 連携をまとめ、導入後すぐ作業流れを使える形にしている。
owox-harness の target harness 設計で、導入体験と命令体系の参考になる。

## 公開値

2026-06-12 時点:

- GitHub 星数: 約 23.3k
- 分岐数: 約 2k
- 履歴数: 345
- 命令: 30 個以上
- agent: 20 個以上
- モード: 7 個
- MCP 連携: 8 種

## 何がすごいか

- Claude Code 利用者へ直撃する導入体験
- 命令、agent、モードを同じ体系に置く
- MCP 連携を任意機能として束ねる
- 役割別 agent の名前と用途が分かりやすい
- インストールと利用開始が速い

## owox-harness が取り入れる点

- target harness の命令一覧
- 作業モードの明示
- agent 役割の名前付け
- 任意 MCP 連携の選択式導入
- 導入後に最初に打つ命令の設計

## owox-harness が超える点

### 複数 AI CLI

SuperClaude Framework は Claude Code 中心。
owox-harness は target harness を複数 AI CLI へ出す。

### 正本検証

SuperClaude Framework は作業流れを強くする。
owox-harness は正本が壊れていないか検証する。

### 来歴

SuperClaude Framework は使う命令が中心。
owox-harness は人間判断、生成、変更、完了の理由を来歴に残す。

### 生成物分離

SuperClaude Framework は導入物を使いやすく置く。
owox-harness は生成物と正本を分け、再生成と移植を前提にする。

## owox-harness が負けている点

- Claude Code 利用者への近さ
- 命令の見つけやすさ
- agent 役割の量
- 導入の即効性
- 公開利用者と認知

## 勝つための判断

SuperClaude Framework は「Claude Code を強く使う道具」で強い。
owox-harness は「複数 AI CLI で同じ判断と正本を守る道具」で勝つ。

命令体系は参考にする。
ただし Claude Code 専用の便利機能へ寄りすぎない。

## 見直し条件

- SuperClaude Framework が複数 AI CLI 対応へ広がった時
- SuperClaude Framework が正本検証を持った時
- SuperClaude Framework が来歴や人間ゲートを持った時
- SuperClaude Framework が target harness 生成へ寄った時

## 参照

- docs/competitive/20260612-競合候補一覧.md
- docs/decisions/20260611-第1対象CLI.md
- docs/decisions/20260611-MCP設計.md
- https://github.com/SuperClaude-Org/SuperClaude_Framework
