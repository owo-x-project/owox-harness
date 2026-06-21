# Claude Flow / ruflo

## 調査日

2026-06-12

## 分類

作業流れ、複数実行エージェント管理。

Claude Code、Codex などとつながる複数 agent 制御基盤。

## 概要

Claude Flow は ruflo へ移行している。

複数 agent、記憶、通信、検証、プラグインを広く扱う。
owox-harness から見ると、勢いと機能量が最も大きい周辺競合の 1 つ。

## 公開値

2026-06-12 時点:

- GitHub 星数: 約 59k
- 分岐数: 約 6.8k
- 履歴数: 6,705
- 公開版: 1,529
- AGENTS.md、CLAUDE.md、`.agents/`、`.claude/`、`.githooks/`、`verification/` を含む

## 何がすごいか

- 公開利用者と勢いが大きい
- 複数 agent 制御へ強く寄っている
- 記憶、通信、検証、プラグインを同じ箱に入れている
- Claude Code と Codex を同時に意識している
- 導入、実行、管理、拡張まで範囲が広い

## owox-harness が取り入れる点

- 複数 agent を前提にした状態管理
- 記憶と検証を分けて置く構成
- `.agents/` などの配置慣習
- 作業流れの拡張点
- Codex 対応を明示する見せ方

## owox-harness が超える点

### 小さく保つ

ruflo は広い。
owox-harness は正本生成、検証、人間判断に絞る。

### 人間判断

ruflo は複数 agent 自律を強く出す。
owox-harness は重要判断を人間が握ることを旗にする。

### 型付き正本

ruflo は機能量が強い。
owox-harness は正本を型で守り、壊れた時に検出する。

### 来歴

ruflo は agent 実行の量で強い。
owox-harness は判断、完了、破棄、例外を来歴へ残す。

## owox-harness が負けている点

- 公開利用者
- 機能量
- 複数 agent 制御
- 記憶機能
- 導入済み連携

## 勝つための判断

ruflo と機能量で殴り合わない。

owox-harness は「人間判断を正本と来歴に残し、複数 AI CLI に移せる」点へ絞る。
ruflo は幅、owox-harness は統制で勝つ。

## 見直し条件

- ruflo が型付き正本を中心に置いた時
- ruflo が人間ゲートを明確化した時
- ruflo が target harness 生成へ寄った時
- ruflo が owox-harness の経験 export / import と重なった時

## 参照

- docs/competitive/20260612-競合候補一覧.md
- docs/decisions/20260611-製品戦略.md
- docs/decisions/20260611-設計原則.md
- https://github.com/ruvnet/claude-flow
- https://github.com/ruvnet/ruflo
