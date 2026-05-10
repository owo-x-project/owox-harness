# AI エージェントハーネス テンプレート

このリポジトリは、人間が技術判断を担い、AI がその判断を速く正確に実行するための汎用ハーネステンプレートです。

単一プロジェクトとモノレポのどちらにも導入できるように、ハーネス本体とプロジェクト正本を分離し、後からプロジェクト固有情報を初期化できる構造にしています。

## 目的

- 人間が判断し、AI はその判断に沿って実行する
- 要件、仕様、判断理由、検証方針を分離して管理する
- AI が参照すべき資料の入口を短く保つ
- 変更理由と影響範囲を後から追える状態を作る

## ディレクトリの考え方

### `.agents/`

AI 専用領域です。プロジェクト定義、task 文脈キャッシュ、skill、補助スクリプトを置きます。

### `docs/project/`

人間向けのプロジェクト正本です。要求、仕様、判断、用語、検証、外部連携、チーム資料を置きます。

クローン直後のこのテンプレートでは、`docs/project/` は空に近いひな型だけを持ちます。プロジェクト固有の内容は `harness-init` 実行後に追加してください。

## 使い始め方

1. テンプレートを対象リポジトリに配置する
2. 必要なら `.agents/scripts/sync_agent.sh` で利用するエージェント向け設定を同期する
3. `harness-init` skill を使って `.agents/project.md` と必要な正本を初期化する
4. 以後は `task-*` と `docs-update-*` を使って作業を進める

## エージェント設定の同期

```bash
bash .agents/scripts/sync_agent.sh <target> [--dry-run] [--force] [--snapshot-name <name>]
```

対応 target:

- `claude`
- `copilot`
- `cursor`
- `opencode`
- `crush`
- `gemini`
- `aider`
- `kiro`

同期前の状態へ戻すには次を使います。

```bash
bash .agents/scripts/restore_agent.sh [--snapshot <snapshot-id>] [--dry-run]
```

## 典型的な流れ

1. 人間が要求や制約を正本として定義する
2. AI は `AGENTS.md` と `.agents/project.md` を入口に必要な資料だけ読む
3. 実装や修正に伴って正本も更新する
4. 検証後に task を閉じ、次に読むべき資料を残す
5. `validate_harness.sh` でハーネス全体の整合性を確認する

## ライセンス

このリポジトリ本体は `LICENSE` の条件で提供されます。

一部の skill は第三者プロジェクトをもとにしています。第三者ライセンス表記は `NOTICE` を参照してください。
