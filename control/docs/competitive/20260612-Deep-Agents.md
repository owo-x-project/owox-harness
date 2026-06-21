# Deep Agents

## 調査日

2026-06-12

## 分類

agent 実行基盤。

長い作業を行う agent の部品をまとめる。

## 概要

Deep Agents は、subagents、file 操作、文脈管理、shell、永続記憶、人間確認、skills、MCP を含む agent 実行基盤。

owox-harness と名前の対象が近い。
ただし Deep Agents は agent を動かす基盤、owox-harness は target harness 正本を作る道具。

## 公開値

2026-06-12 時点:

- GitHub 星数: 約 24.5k
- 分岐数: 約 3.5k
- 履歴数: 2,190
- ライセンス: MIT

## 何がすごいか

- agent 実行に必要な要素を一式で持つ
- subagents、文脈管理、永続記憶、人間確認を明示する
- MCP を持ち込み可能にしている
- 既存の LangGraph / LangSmith へ接続する
- 実行 agent の本体として使える

## owox-harness が取り入れる点

- agent 実行に必要な要素の整理
- 人間確認を中核機能として置くこと
- subagents と文脈分離
- skills の遅延読み込み
- MCP を道具として開く設計

## owox-harness が超える点

### repo 正本

Deep Agents は agent を動かす。
owox-harness は repo に置く target harness 正本を作る。

### 生成物分離

Deep Agents は実行基盤。
owox-harness は正本、生成物、経験、来歴を分ける。

### 検証可能性

Deep Agents は agent 能力が強い。
owox-harness は正本が壊れた時、task が腐った時、検証なし完了した時に止める。

## owox-harness が負けている点

- 実行基盤の厚み
- subagent 機構
- 永続記憶
- 人間確認の実装済み度
- LangChain 由来の認知と周辺基盤

## 勝つための判断

Deep Agents と「agent 実行基盤」で競わない。

owox-harness は Deep Agents のような基盤に渡すルール、文脈、制約、検証、task を作る。
もし連携するなら、owox-harness は上位の正本生成と検証を担当する。

## 見直し条件

- Deep Agents が repo へ target harness 正本を生成し始めた時
- Deep Agents が AGENTS.md / skills / hooks 生成を扱った時
- Deep Agents が task 正本と腐敗検知を持った時
- owox-harness が agent 実行基盤を内包したくなった時

## 参照

- docs/competitive/20260612-競合候補一覧.md
- docs/decisions/20260611-制御方針.md
- docs/decisions/20260611-target-harness内容.md
- https://github.com/langchain-ai/deepagents
