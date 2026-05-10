# プロジェクト資料

## 目的

`docs/project/` は、このプロジェクトの正本を置く場所です。要求、仕様、計画、判断、用語、検証、外部連携、チーム資料をここで管理します。

## 読む順序

1. `glossary/core.md`
2. `architecture.md`
3. 対象カテゴリの `index.md`
4. 必要な個票

## 資料種別の使い分け

- `research/`: 技術調査、制約調査、実現性確認
- `proposals/`: 正式化前の設計草案、提案、移行案
- `plan/`: 実装計画、ロードマップ、検証可能なフェーズと task
- `requirements/`: 何を実現するか
- `specs/`: どう振る舞うか
- `adr/`: 重要判断と理由
- `patterns/`: 横断的に再利用する設計・実装パターン

## AI 参照導線

AI は通常、この index から `glossary/core.md`、`architecture.md`、対象カテゴリ `index.md`、対象個票の順で参照します。archive は明示的に必要な場合だけ参照します。

## 参照

- `glossary/index.md`: 用語集カテゴリの入口
- `glossary/core.md`: 必読の共通用語
- `architecture.md`: 不変条件、責務分離、設計方針
- `research/index.md`: 技術調査カテゴリの入口
- `proposals/index.md`: 設計草案カテゴリの入口
- `plan/index.md`: 実装計画カテゴリの入口
- `requirements/index.md`: 要求カテゴリの入口
- `specs/index.md`: 仕様カテゴリの入口
- `patterns/index.md`: パターンカテゴリの入口
- `adr/index.md`: 重要判断カテゴリの入口
- `teams/index.md`: チームガイドカテゴリの入口
- `integrations/index.md`: 外部連携カテゴリの入口
- `tech-stack.md`: 採用技術一覧
- `validation.md`: 検証方針
