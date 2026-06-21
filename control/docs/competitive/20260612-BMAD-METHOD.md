# BMAD-METHOD

## 調査日

2026-06-12

## 分類

作業流れ。

AI 開発の段階、役割、専門エージェントを定義する。

## 概要

BMAD-METHOD は、AI 駆動開発を段階と役割で進める方法。

企画、分析、設計、実装、確認を分け、専門エージェントに役割を与える方向。
owox-harness にとって、target harness の導入体験と役割分担の参考になる。

## 公開値

2026-06-12 時点:

- GitHub 星数: 約 49k
- 分岐数: 約 5.7k
- 公開版数: 36
- 最新公開版: 2026-05-25
- ライセンス: MIT

## 何がすごいか

- 役割が分かりやすい
- 段階の名前が人間に伝わる
- 導入時に「何をすればよいか」が見えやすい
- 専門エージェントの使い方が具体的
- AI 開発を一人の会話で終わらせず、工程へ分解する

## owox-harness が取り入れる点

- 段階ごとの入口
- 役割名の分かりやすさ
- 専門 agent の責務分離
- 導入直後の次行動
- 作業流れの説明順

## owox-harness が超える点

### 機械強制

BMAD は方法として強い。
owox-harness は hooks、MCP、型付き正本で止める。

### 正本所有

BMAD は作業流れと役割が主軸。
owox-harness は target harness 正本そのものを所有し、生成物と分ける。

### 検証

BMAD は段階で品質を上げる。
owox-harness は完了3区別、証跡、検証 gate を構造に入れる。

## owox-harness が負けている点

- 説明の分かりやすさ
- 役割名の強さ
- 導入体験
- 利用者が真似しやすい作業流れ
- コミュニティ認知

## 勝つための判断

BMAD の良さは「人間が理解できる工程」。
owox-harness はそこを取り入れつつ、「工程を守れなかった時に止まる」ことを差別化にする。

## 見直し条件

- BMAD が機械検証を中核にした時
- BMAD が MCP tool / resource を主入口にした時
- BMAD が task 腐敗検知を持った時
- BMAD が複数 AI CLI 向け target harness 生成へ寄った時

## 参照

- docs/competitive/20260612-競合候補一覧.md
- docs/decisions/20260611-オーケストレーション.md
- docs/decisions/20260611-製品戦略.md
- https://github.com/bmad-code-org/BMAD-METHOD
