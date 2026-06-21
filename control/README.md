# owox-harness

`owox-harness` は、AIエージェントを使った開発で、人間が重要な判断を握りながら、AIの自律性を活かすための harness正本を作成・管理する道具。

このリポジトリは `control repo`。`owox-harness` の開発と検証を行う。

## 導入

GitHub Releases から `owox` 実行ファイルを取得する。導入スクリプトが checksum を照合してから配置する。

linux / macOS:

```sh
curl -fsSL https://raw.githubusercontent.com/owoDra/workspace/main/control/scripts/setup.sh | sh
```

Windows (PowerShell):

```powershell
irm https://raw.githubusercontent.com/owoDra/workspace/main/control/scripts/install.ps1 | iex
```

- 既定の配置先は linux / macOS が `~/.local/bin`、Windows が `%LOCALAPPDATA%\owox\bin`。環境変数 `OWOX_BIN_DIR` で変えられる
- 版を固定する時は `OWOX_VERSION=owox-v0.1.0` を渡す。無指定なら最新の `owox-v*`
- 配布対象は linux (x86_64 / arm64)・macOS (Apple Silicon)・Windows (x86_64)
- 導入後に `owox --version` で版を確認できる

`owox` は常駐しない。利用は MCP が主入口で、`owox setup` が対象リポジトリへ設定を生成する。

## 開発の進め方

1. 要件を `docs/requirements` に残す
2. 設計判断を `docs/decisions` に残す
3. 実装する
4. `target repo` で検証する
5. 検証結果を `docs/validation` に残す

## 文書配置

- `AGENTS.md`: control harness の最上位正本
- `CLAUDE.md`: Claude Code 用の control harness 入口
- `.claude/settings.json`: Claude Code 共有設定
- `.devcontainer`: Claude Code 導入済み開発コンテナ設定
- `README.md`: 人間向け説明
- `docs/requirements`: 要件、実装計画、完了条件
- `docs/concept`: 理念、背景、理想像
- `docs/decisions`: 採用判断、技術選定、見直し条件
- `docs/validation`: target repo での検証結果
- `docs/handoff`: 作業引き継ぎ。git 管理対象外

## 読み方

最初に読むもの:

- `AGENTS.md`
- `README.md`
- 必要な `docs/*/INDEX.md`

必要な時だけ読むもの:

- 目的や理想像を確認する時: `docs/concept`
- 何を作るか確認する時: `docs/requirements`
- なぜその設計か確認する時: `docs/decisions`
- 動作確認結果を見る時: `docs/validation`
