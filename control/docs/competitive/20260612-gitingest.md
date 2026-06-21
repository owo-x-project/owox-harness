# gitingest

## 調査日

2026-06-12

## 分類

文脈管理、repo 圧縮。

GitHub URL から AI 向け文脈をすぐ作る。

## 概要

gitingest は、GitHub URL から repo 概要と内容を AI 向けに抽出する道具。

Web、CLI、Python から使える。
URL を変えるだけで使える導線が強い。

## 公開値

2026-06-12 時点:

- GitHub 星数: 約 14.9k
- 分岐数: 約 1.1k
- 履歴数: 405
- Web、CLI、Python から使える
- 旧 `cyclotruc/gitingest` から `coderamp-labs/gitingest` へ転送を確認

## 何がすごいか

- URL だけで使える
- 初回導入の説明が短い
- repo を AI 向け入力へすばやく変換する
- Web と CLI の両方を持つ
- OSS 調査時の導線が分かりやすい

## owox-harness が取り入れる点

- URL から文脈を作る軽い導線
- 調査用文脈の即時生成
- Web と CLI の両導線
- 出力の分かりやすさ
- 初回体験の短さ

## owox-harness が超える点

### 正本との接続

gitingest は文脈を作る。
owox-harness は文脈を task、検証条件、来歴へ結ぶ。

### 範囲制御

gitingest は簡単に広く取り込める。
owox-harness は必要最小限の文脈へ絞る。

### 安全

gitingest は調査入力が速い。
owox-harness は秘密情報、人間ゲート、除外規則を重視する。

### 継続運用

gitingest はその場の文脈生成が強い。
owox-harness は文脈の古さや再取得理由を管理する。

## owox-harness が負けている点

- URL だけで使える導線
- Web 体験
- 調査速度
- 文脈抽出の手軽さ
- 説明の短さ

## 勝つための判断

gitingest は「とにかく早く repo を読む」で強い。
owox-harness は「読んだ文脈を正本と判断へつなげる」で勝つ。

競合調査や target repo 初期把握では参考にする。

## 見直し条件

- gitingest が task 単位文脈を持った時
- gitingest が検証条件と結びついた時
- gitingest が target harness 生成へ寄った時
- gitingest が秘密情報検査や人間ゲートを強化した時

## 参照

- docs/competitive/20260612-競合候補一覧.md
- docs/decisions/20260611-品質保証.md
- https://github.com/coderamp-labs/gitingest
- https://github.com/cyclotruc/gitingest
