# vibe-kanban

## 調査日

2026-06-12

## 分類

タスク管理、実行エージェント管理。

kanban と作業領域で、複数実行エージェントを管理する。

## 概要

vibe-kanban は、Claude Code、Gemini CLI、Codex、Amp などの実行エージェントから成果を引き出すための kanban。

作業を issue として計画し、実行時は作業領域を作る。
owox-harness にとって、複数実行エージェント管理の見せ方が参考になる。

## 公開値

2026-06-12 時点:

- GitHub 星数: 約 26.9k
- 分岐数: 約 2.8k
- 公開版数: 284
- 最新公開版: 2026-04-24
- ライセンス: Apache-2.0
- 注意: 公開 README 上で終了予定あり

## 何がすごいか

- 実行エージェントの作業を kanban で見られる
- 計画と確認に焦点がある
- 複数実行エージェントを同じ作業面で扱う
- worktree / 作業領域の発想が実務に近い
- 非同期作業の体験が分かりやすい

## owox-harness が取り入れる点

- task と作業領域の対応
- 複数実行エージェントの一覧性
- 差分確認を人間の主作業に置く設計
- 計画と確認を速くする見せ方
- 作業ごとの隔離

## owox-harness が超える点

### 正本

vibe-kanban は管理画面と作業体験が強い。
owox-harness は repo 内の target harness 正本を強くする。

### 検証 gate

vibe-kanban は作業を進める。
owox-harness は検証がない完了を閉じない。

### 継続性

vibe-kanban は終了予定がある。
owox-harness は正本形式と export / import を重視し、道具に閉じない。

## owox-harness が負けている点

- 画面体験
- 複数実行エージェントの見える化
- kanban と作業領域の直感
- 非同期作業の分かりやすさ
- 実利用イメージ

## 勝つための判断

vibe-kanban は「見える作業管理」で強い。
owox-harness はまず画面で競わない。

差別化は、task 正本、検証 gate、来歴、人間判断を repo に残せること。
将来の表示層は、この調査票を参考にする。

## 見直し条件

- vibe-kanban の終了方針が変わった時
- 後継が出た時
- task 正本、検証 gate、MCP 連携が強くなった時
- owox-harness が UI を持つ時

## 参照

- docs/competitive/20260612-競合候補一覧.md
- docs/decisions/20260611-タスク管理.md
- https://github.com/BloopAI/vibe-kanban
