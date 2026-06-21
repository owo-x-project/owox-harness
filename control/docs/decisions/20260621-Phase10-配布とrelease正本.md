# Phase10 配布と release 正本

## 状態

採用 (実装中)。

## 背景

Phase10 D群。製品を他者が導入できる形にする。配布方式は技術スタックで既定済み (`docs/decisions/20260611-技術スタック.md`): GitHub Releases + setup.sh + checksum 検証。Phase5 で「バイナリ install は後フェーズ」と保留した分 (`crates/mcp/src/setup.rs` 冒頭) をここで実装する。

あわせて、配布運用がある対象プロジェクト向けの任意正本 release.toml (配布方針 / 版 / 成果物検証。`docs/decisions/20260611-target-harness内容.md`) を型確定する。

owox-harness 自身の配布と、対象プロジェクトの配布支援 (release.toml) は別物。両方とも「成果物の完全性を機械で確かめる」点で同じ思想に立つ。

## 決めること

- owox-harness 自身の配布: ビルド対象・成果物形式・checksum・導入スクリプト
- 版の持ち方
- 対象プロジェクト向け release.toml の型と検証

## 採用案

### 配布 (owox-harness 自身)

- 配置: モノレポ root (`/workspace/product`) の `.github/workflows/release.yml`。tag は `owox-v*` で owox-harness 配布を他成果物から分離。ビルドは `control/` から
- ビルド対象4種 (すべて native runner でクロスビルドを避ける)
    - x86_64-unknown-linux-gnu
    - aarch64-unknown-linux-gnu
    - aarch64-apple-darwin
    - x86_64-pc-windows-msvc
- 成果物: `owox-<target>.tar.gz` (unix) / `owox-<target>.zip` (windows)。1成果物に owox 実行ファイル1つ
- 完全性: 全成果物の SHA256 を1つの `SHA256SUMS` へまとめ Release へ添付
- 導入スクリプト
    - setup.sh: unix (linux / macOS) 用。OS と CPU を判定 → 対応成果物と SHA256SUMS を取得 → checksum 照合 → 実行ファイルを導入先へ配置。導入先は既定 `~/.local/bin`・環境変数で変更可
    - install.ps1: Windows 用。同手順を PowerShell で
- 版: workspace 共通 version を `0.1.0` から開始。owox に `--version` / `-V` を追加し導入物の版を確認可能に

### release.toml (対象プロジェクト向け任意正本)

`.owox/release.toml`。配布運用がある対象プロジェクトだけ置く。無くても生成が通る (他の任意正本と同じ)。3要素:

- 配布方針 (`policy`): 文字列配列。人間向けの方針メモ。機械強制せず案内のみ
- 版 (`[version]`): `file` (版が書かれたファイル) + `pattern` (版を取り出す正規表現・捕捉群1つ・行頭マッチは複数行モード `(?m)` を付ける)。owox が現在の版を読み取る
- 成果物検証 (`[[artifacts]]` と `[release] checks`): `[[artifacts]]` は期待する成果物名の列挙 (owox が dist ディレクトリ内の存在を確認)。`[release] checks` は checksum / 署名など実検証を対象プロジェクトのコマンドへ委譲 (verify.checks・decay.checks と同じ委譲方式)

検証は `release.check` MCP tool: release.toml を読み、版を抽出し、dist ディレクトリの成果物存在を確認し、委譲検査を実行して封筒で返す。owox 自身が hash を計算せず、検査は対象プロジェクトへ委譲する (言語非依存・構文解析を持たない方針と一貫。`crates/core/src/quality.rs` 冒頭と同じ)。

## 理由

- native runner 4種はクロスビルドの罠 (リンカ・sysroot) を避け、保守が軽い。GitHub は linux arm runner を公開済みで aarch64-linux も native で組める
- 1つの SHA256SUMS は setup.sh の照合を単純化し、成果物追加でスクリプトを増やさない
- tag prefix `owox-v*` はモノレポの既存 tag (`v1`) と衝突せず、owox-harness 配布を独立して切れる
- 導入先既定 `~/.local/bin` は sudo 不要で個人開発者 (第一対象) に素直
- release.toml の検査委譲は、owox が hash 計算の新依存を抱えず、対象プロジェクトの実情 (cosign / minisign / sha256sum) に合わせられる。版の取り出しだけ正規表現で owox が担う (regex は既存依存)
- owox 自身の配布も対象プロジェクトの release.toml も「成果物の完全性を確かめる」で揃う。安全性を要件領域に持つ製品として一貫する

## 捨てた案

- owox が成果物の SHA256 を Rust で計算する (sha2 等の新依存)
- macOS x86_64・Windows arm のビルド
- musl 静的リンクでの linux ビルド
- 成果物ごとに個別 checksum ファイル
- release.toml を必須正本にする
- 配布物への署名 (cosign / minisign)

## 捨てた理由

- hash の新依存は依存追加条件に見合わず、検査委譲で代替できる
- 対象を絞り native runner 4種に留めて保守を軽くする。需要が出たら見直す
- musl は rmcp / tokio との組み合わせ検証が要り初期に重い。gnu で先に出す
- 個別 checksum ファイルは setup.sh の取得とスクリプトを増やす
- release.toml 必須化は配布運用の無い対象プロジェクトに無用な正本を強いる
- checksum で当面足り、署名は技術スタックの見直し条件に従い必要時に足す

## 見直し条件

- gnu ビルドが古い glibc の環境で動かない報告が出た時 (musl へ)
- checksum で足りず署名が必要になった時 (技術スタックの見直し条件と同じ)
- Windows arm / macOS x86_64 の需要が出た時
- release.toml の成果物検証で委譲だけでは粗く、owox 内 hash 計算が要ると分かった時
- モノレポから control を独立リポジトリへ切り出す時 (.github の配置を見直す)

## 未決事項

- release.check の実機検証 (対象プロジェクトでの dist 照合)
- GitHub Releases への実 tag push による workflow の実走確認 (人間関与)
