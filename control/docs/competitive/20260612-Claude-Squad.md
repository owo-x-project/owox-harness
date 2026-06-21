# Claude Squad

## 調査日

2026-06-12

## 分類

実行エージェント管理。

実行エージェントそのものではなく、複数の端末 agent 作業領域を束ねる。

## 概要

Claude Squad は、Claude Code、Codex、Gemini、Aider などの作業セッションを 1 つの端末画面で管理する道具。

tmux と git worktree を使い、複数 agent を別々の作業領域で動かす。
変更確認、checkout、push 前確認を重視している。

## 公開値

2026-06-12 時点:

- GitHub 星数: 約 7.8k
- 分岐数: 552
- 履歴数: 216
- 公開版: 18
- 最新公開版: 2026-05-23
- tmux と git worktree を前提にする

## 何がすごいか

- 複数 agent セッション管理が具体的
- 作業領域分離が git worktree で分かりやすい
- 変更確認後に取り込む流れがある
- 対象 agent を命令文字列で差し替えられる
- 端末だけで利用できる

## owox-harness が取り入れる点

- 複数作業領域を分ける考え方
- 差分確認後に取り込む作業流れ
- 実行エージェント別の起動設定
- セッション状態を一覧する体験
- push 前確認の標準化

## owox-harness が超える点

### 正本

Claude Squad はセッション管理が中心。
owox-harness は target harness 正本を生成、管理、検証する。

### task 完了

Claude Squad は作業を並列化する。
owox-harness は task が検証なしに完了扱いにならないよう守る。

### 来歴

Claude Squad は変更取り込みを助ける。
owox-harness は判断と結果の理由を残す。

### 移植

Claude Squad はローカル実行管理。
owox-harness は target harness を別 repo、別 AI CLI へ移す前提を持つ。

## owox-harness が負けている点

- 複数セッション管理
- worktree 運用
- 端末体験
- 既存 agent への接続しやすさ
- 変更確認の手軽さ

## 勝つための判断

Claude Squad は「複数 agent を同時に走らせる」体験で強い。
owox-harness は「走らせる前後の判断、正本、検証」で勝つ。

実行管理は作り込みすぎない。
ただし作業領域分離と差分確認は target harness の重要参考にする。

## 見直し条件

- Claude Squad が task 正本を持った時
- Claude Squad が検証ゲートを持った時
- Claude Squad が target harness 生成へ寄った時
- Claude Squad が来歴管理を持った時

## 参照

- docs/competitive/20260612-競合候補一覧.md
- docs/decisions/20260611-タスク管理.md
- docs/decisions/20260611-品質保証.md
- https://github.com/smtg-ai/claude-squad
