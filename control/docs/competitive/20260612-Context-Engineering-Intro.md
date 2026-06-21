# Context Engineering Intro

## 調査日

2026-06-12

## 分類

文脈管理、仕様駆動。

AI に渡す文脈を設計し、実装前の材料を整える。

## 概要

Context Engineering Intro は、AI 開発で失敗しにくい文脈を作るための雛形。

CLAUDE.md、例、初期依頼、PRP を使い、実装前に必要情報と検証流れを整える。
owox-harness の文脈設計、要件化、検証前提の説明で参考になる。

## 公開値

2026-06-12 時点:

- GitHub 星数: 約 13.5k
- 分岐数: 約 2.7k
- 履歴数: 25
- CLAUDE.md、PRPs、examples、INITIAL.md を含む

## 何がすごいか

- 失敗理由を「文脈不足」として分かりやすく説明する
- 例を文脈の一部として扱う
- 実装前に PRP を作る流れが明確
- 検証を最初から入れる
- 使い始めるまでが軽い

## owox-harness が取り入れる点

- 文脈を成果物として扱う説明
- 例を正本近くに置く考え方
- 初期依頼から実装計画へ進む流れ
- 検証条件を先に書く流れ
- 文脈不足を検知する観点

## owox-harness が超える点

### 型付き正本

Context Engineering Intro は雛形。
owox-harness は正本を型で守る。

### 複数 AI CLI

Context Engineering Intro は Claude Code 寄り。
owox-harness は複数 AI CLI へ target harness を出す。

### 腐敗検知

Context Engineering Intro は良い文脈を作る。
owox-harness は古くなった文脈、孤立した文脈、検証なし完了を検出する。

### 来歴

Context Engineering Intro は実装前準備が強い。
owox-harness は判断と変更理由を継続的に残す。

## owox-harness が負けている点

- 説明の分かりやすさ
- 雛形の軽さ
- 文脈設計の教材性
- 初回導入の速さ
- PRP の認知

## 勝つための判断

Context Engineering Intro は「良い依頼と文脈」で強い。
owox-harness は「文脈を正本化し、検証し、腐敗を見つける」で勝つ。

文脈設計の説明は大きく参考にする。
ただし雛形配布だけで止めない。

## 見直し条件

- Context Engineering Intro が道具化した時
- Context Engineering Intro が複数 AI CLI へ正式対応した時
- Context Engineering Intro が文脈の検証や腐敗検知を持った時
- Context Engineering Intro が target harness 生成へ寄った時

## 参照

- docs/competitive/20260612-競合候補一覧.md
- docs/requirements/20260611-core-要件.md
- docs/decisions/20260611-設計原則.md
- https://github.com/coleam00/context-engineering-intro
