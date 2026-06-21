# GitHub Spec Kit

## 調査日

2026-06-12

## 分類

仕様駆動。

仕様、計画、task、実装の流れを repo に置く。

## 概要

GitHub Spec Kit は、仕様駆動開発を始めるための道具。

中心は「仕様を先に置き、計画と task を作り、実装へ進む」流れ。
owox-harness と同じく、曖昧な会話を repo 内の正本に落とす方向。

## 公開値

2026-06-12 時点:

- GitHub 星数: 約 112k
- 分岐数: 約 9.8k
- 公開版数: 162
- 最新公開版: 2026-06-11
- ライセンス: MIT

## 何がすごいか

- GitHub 公式で認知が大きい
- 仕様駆動という言葉を広げる力が強い
- 仕様、計画、task、実装の段階が分かりやすい
- 憲法という上位制約を持つ
- AI 開発を「思いつき実装」から「仕様→実装」へ戻す

## owox-harness が取り入れる点

- 仕様を入口にする導入体験
- 憲法に近い上位制約
- 仕様、計画、task の分離
- 最初の数分で価値が分かる雛形
- README で伝わる短い概念説明

## owox-harness が超える点

### 検証可能な正本

Spec Kit は仕様駆動の体験が強い。
owox-harness は仕様だけでなく正本全体を検証対象にする。

### 人間ゲート

Spec Kit は仕様から実装へ進める。
owox-harness は不可逆操作、範囲外変更、正本昇格を止める。

### 完了3区別

Spec Kit の task は実装流れとして強い。
owox-harness は作業完了、要件完了、検証完了を分ける。

### 腐敗検知

Spec Kit は仕様と task を作る。
owox-harness は古い、孤立、重複、未検証 done を検出する。

## owox-harness が負けている点

- 認知
- GitHub 公式性
- 星数
- 導入の分かりやすさ
- 仕様駆動という市場での言葉の強さ

## 勝つための判断

Spec Kit と「仕様駆動の分かりやすさ」で正面衝突すると負けやすい。

owox-harness は「仕様も含む、検証可能な target harness 正本」で勝つ。
実演は「仕様はあるが、検証・人間判断・task 腐敗が抜けると危ない」を見せる。

## 見直し条件

- Spec Kit が MCP 主入口を持った時
- Spec Kit が hooks / skills / subagents を含む target harness 生成へ寄った時
- Spec Kit が完了3区別や腐敗検知を持った時
- GitHub 側の実行エージェント管理と統合された時

## 参照

- docs/competitive/20260612-競合候補一覧.md
- docs/decisions/20260611-製品戦略.md
- docs/decisions/20260611-品質保証.md
- https://github.com/github/spec-kit
