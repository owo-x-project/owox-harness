# Task Master

## 調査日

2026-06-12

## 分類

タスク管理。

AI 開発用の task、依存、研究、実装列を扱う。

## 概要

Task Master は、AI 駆動開発向けのタスク管理。

MCP を推奨入口にし、Cursor、Windsurf、VS Code、Claude Code、Codex CLI などとつなぐ。
owox-harness の task 設計に近い競合。

## 公開値

2026-06-12 時点:

- GitHub 星数: 約 27.4k
- 分岐数: 約 2.6k
- 履歴数: 1,216
- MCP 設定例あり
- CLI 命令参照あり

## 何がすごいか

- MCP 主入口をすでに持つ
- task 依存、構造、作業列を前面に出している
- 複数 AI CLI / editor へ広げている
- 研究用 model と主 model を分ける
- task 管理を AI 作業の中心に置く

## owox-harness が取り入れる点

- MCP から task を操作する体験
- task 依存の見せ方
- 研究と実装の分離
- 複数 AI CLI への導入案内
- task 構造の説明資料

## owox-harness が超える点

### done 検証

Task Master は task を進める体験が強い。
owox-harness は done を自己申告にしない。

### 来歴連動

Task Master は task 管理が中心。
owox-harness は task の生成、完了、破棄の理由を来歴へ結ぶ。

### 腐敗検知

Task Master は依存と作業列を扱う。
owox-harness は放置、孤立、重複、未検証完了、永久停止を機械検出する。

### 人間ゲート

Task Master は実行を助ける。
owox-harness は危険な task、不可逆 task、範囲外 task を needs_human にする。

## owox-harness が負けている点

- 既存利用者
- MCP task 体験
- 対応環境
- task 管理資料
- 導入の即効性

## 勝つための判断

Task Master は「AI が迷わない task 管理」で強い。
owox-harness は「嘘 done と task 腐敗を許さない task 正本」で勝つ。

Beads と同じく、task 層では最重要比較対象。

## 見直し条件

- Task Master が done 検証必須を導入した時
- Task Master が task 腐敗検知を導入した時
- Task Master が来歴と task を強く結んだ時
- Task Master が target harness 生成へ寄った時

## 参照

- docs/competitive/20260612-競合候補一覧.md
- docs/decisions/20260611-タスク管理.md
- docs/decisions/20260611-MCP設計.md
- https://github.com/eyaltoledano/claude-task-master
