# owox-harness

owox-harness は、AI を使った開発を、迷いにくく、検証しやすい流れに変えるための道具です。

AI に、プロジェクトの規則、次にやる作業、完了を証明する確認、そして人間が判断すべき場所を渡します。
長い指示文で毎回説明するのではなく、コンテキスト、規則、要件、判断、学びをプロジェクトの正本として残し、対応している AI ツール向けに生成します。

目指すのは、AI も人間も「次に何をするか」「なぜそれが必要か」「どう確認すれば完了か」で迷わない状態です。

Language: 日本語 | [English](./README.md)

## 考え方

owox-harness は、AI に本格的な作業を任せたいが、プロジェクトをチャット履歴、古い指示文、曖昧な判断だらけにしたくない人のための道具です。

重視する成果は五つです。

- **次にやることが常に見える。** 未判断の事項、着手できる作業、読むべきコンテキスト、必要な確認を示し、人間も AI も毎回手探りしなくてよくします。
- **進捗を宣言し、検証できる。** 要件、作業、判断、確認を結び付けます。AI が「終わった」と言うだけではなく、何を満たしたから終わりなのかを示せます。
- **必要最小限のコンテキストで動ける。** 今の作業に必要な情報だけを渡します。余計な情報を減らし、規則の見落としや思い込みを減らします。
- **プロジェクトに合わせて育つ。** 新しい規則、学び、危険、スキル、確認を記録し、次の作業へ反映します。
- **経験をほかのプロジェクトへ持ち出せる。** 役に立つスキルや学びを、一つのプロジェクトや一つの AI ツールに閉じ込めず、再利用できる形で残します。

owox-harness の中心にある考えは「AI に全部任せる」ことではありません。
「人間が決め、AI が進め、owox-harness が流れを示し、プロジェクトが記憶する」ことです。

現在の対応先:

- Codex CLI
- Claude Code

## 導入

GitHub Releases から `owox` 実行ファイルを導入します。

Linux と macOS:

```sh
curl -fsSL https://raw.githubusercontent.com/owoDra/workspace/main/control/scripts/setup.sh | sh
```

Windows PowerShell:

```powershell
irm https://raw.githubusercontent.com/owoDra/workspace/main/control/scripts/install.ps1 | iex
```

導入後、バージョンを確認します。

```sh
owox --version
```

既定の導入先:

- Linux と macOS: `~/.local/bin`
- Windows: `%LOCALAPPDATA%\owox\bin`

導入先を変える場合は `OWOX_BIN_DIR` を指定します。
バージョンを固定する場合は `OWOX_VERSION` を指定します。例: `owox-v0.1.0`

## 基本的な使い方

開発対象のリポジトリで実行します。

```sh
owox setup
```

別の場所にあるリポジトリへ使う場合:

```sh
owox setup path/to/project
```

## リポジトリ構成

この製品リポジトリに含まれるもの:

- `control/`: owox-harness の実装、文書、検証
- `target/`: 検証用の作業場所
- `.github/workflows/`: 配布用の処理

owox-harness の主な文書は `control/docs/` にあります。

## 許諾

MIT License です。詳細は `LICENSE` を参照してください。
