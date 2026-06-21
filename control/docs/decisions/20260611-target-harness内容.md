# target harness 内容

## 状態

採用済み。

## 背景

要件の未決「target harness の具体内容」を決める。
設計原則 (`docs/decisions/20260611-設計原則.md`) を、生成物として何を出すかに落とす。
置き場所・型はアーキ (`docs/decisions/20260611-開発アーキテクチャ.md`) に従う。

## 構成

target harness は4部からなる。

### 1. 正本ソース .owox/

生成のもと。人間と AI が編集する。機能別ファイル+一部ディレクトリで持つ。

```text
.owox/
  brand.toml        目的 (Vision) / 価値・方針 / 対象外・成功条件 / 用語集 / 表記・文体
  rules.toml        作業ルール / 変更方針 / 依存追加条件 / 削除基準 / 可逆性境界 (不可逆操作) / 人間ゲート / 安全性
  quality.toml      品質バー / 依存方向 / 層境界 / 循環禁止 / 複雑度予算
  context.toml      文脈地図 (作業 → 必読 + 必要なら読む)
  state.toml        プロジェクト状態 (初期 / 安定 / 保守)
  agents.toml       subagent 役割・権限・モデルティア・オーケストレーションパターン (`docs/decisions/20260611-オーケストレーション.md`)
  commands.toml     人間ショートカット定義 (`docs/decisions/20260611-ショートカット.md`)
  permissions.toml  役割・承認ポリシー (team。`docs/decisions/20260611-チーム権限.md`)
  config.toml       machine 設定の集約。今は [targets.<cli>] (生成対象 CLI ・出力先・ティア → モデル)
  release.toml      配布方針 / 版 / 成果物検証 (必要な対象プロジェクトだけ)
  skills/           SKILL.md + tests + scripts + 経験メモリ
  requirements/     要件 / 受け入れ基準 / 検証 link
  tasks/            検証可能タスク (id / status / links / deps。`docs/decisions/20260611-タスク管理.md`)
  decisions/        来歴 (ID / 状態 / 根拠 / リンク)
  experience/       汎用経験 (export 対象)
  verification/     検証定義・結果
  work/             作業メモ (作業層メモリ、git 管理対象外)
```

### 一般開発正本の置き方

- 目的、対象外、成功条件は brand.toml に寄せる
- 変更方針、依存追加条件、削除基準は rules.toml に寄せる
- 所有者、承認、責任境界は permissions.toml と CODEOWNERS に寄せる
- 品質バー、依存方向、層境界、複雑度予算は quality.toml に分ける
- 要件、受け入れ基準、検証 link は requirements/ に置く
- 失敗記録は verification / decisions / experience の link で表す

### 2. 生成物

各 AI CLI の規定位置へ出す。派生であり、再生成で壊れない。

- ルート指示ファイル: ブランド+ルール+文脈誘導を最小で集約 (例 AGENTS.md)
- hook 群: context-index / related-files / diff-summary / done-criteria / work-note 相当
- MCP 接続設定
- slash command (人間ショートカット。`docs/decisions/20260611-ショートカット.md`)
- CODEOWNERS (.owox/ 重要パス → maintainer。team。`docs/decisions/20260611-チーム権限.md`)
- skills 配置 (owox 標準 skill: kickoff、判断支援、多視点レビュー。`docs/decisions/20260611-方向付け.md`、`docs/decisions/20260611-品質保証.md`)
- subagents 定義 (役割: 調査 / 計画 / 実装 / レビュー / 検証。権限・モデルティアつき。`docs/decisions/20260611-オーケストレーション.md`)

### 3. owox MCP 操作

- generate / regenerate: target harness 生成
- inspect / diff: 生成内容・差分
- decide: 来歴追記
- requirement: 要件・受け入れ基準・検証 link 管理 (深い仕様管理は対象外)
- verify: 検証実行、完了3区別判定
- gate: 人間判断が必要な点を返す (不可逆操作、正本昇格)
- skill.create / evaluate / register / refine: スキル・ライフサイクル
- experience.export / import: 経験の出入
- state.get / set: プロジェクト状態

### 4. setup

初回導入のみ。バイナリ配置・各 CLI の MCP 設定補助・検査。常駐しない。

## 静的・動的の線引き

- 静的入力 (編集対象): brand / rules / quality / requirements / context / state / targets / skills 定義 / subagents
- 生成物 (派生・再生成可): ルート指示ファイル / hook / skills 配置 / 文脈 index
- 動的 (MCP で追記・実行): 来歴 / 経験 / 検証結果 / スキル・ライフサイクル / gate

## hook の実体

- hook は薄いシェルにし、owox 実行ファイルを一発 (常駐せず) 呼ぶ
- 決定論・型付きロジックは core に集める。シェルへ散らさない
- owox 実行ファイルは、MCP サーバ用途と hook 補助用途を持つ (常駐 CLI ではない)

## 実装順

### 第1段階 (Codex CLI、スキル・ライフサイクルまで)

- 正本ソース: brand (最小、対象外・成功条件を含む) / rules (変更方針・依存追加条件・削除基準・可逆性境界・人間ゲート・安全性) / context / targets
- 生成物: ルート指示ファイル + hook 5種 + Codex MCP 設定 + skills 配置 + slash command (立ち上げ・方向 / 判断・検証 / タスク)
- MCP: generate / inspect・diff / decide / verify / gate / skill (create・evaluate・register・refine)
- 完了3区別のうち要件完了は人間判断 (needs_human)。機械判定は第2段階の要件↔テスト trace 導入後
- スキル・ライフサイクル: 作成 → テスト合格ゲート → 登録 → 改善、経験メモリ
- 記録先: .owox/ の decisions / verification / work / skills
- setup: 配置 + Codex MCP 設定 + 検査

### 第2段階以降

- requirements/ (要件・受け入れ基準・検証 link)
- quality.toml (品質バー・適応度関数定義)
- 経験の export / import (汎用のみ、型でドメイン分離)
- subagents の作り込み
- 適応度関数の作り込み、ブランド機械検証
- 状態適応・回帰防止
- マルチ CLI 生成
- eval の本格導入
- release.toml (配布運用がある対象プロジェクトのみ)

## 機械検証

- 型検証: TOML 必須項目、値の種類、ID 形式、重複なし
- 参照検証: requirement / task / decision / verification の link 存在
- 差分検証: 変更ファイルが rules.toml の許可範囲内か、危険変更に来歴があるか
- 完了検証: done 前に検証結果があり、失敗が残っていないか
- 依存検証: 依存追加に理由、代替案、見直し条件、検証結果があるか
- 文書検証: 公開挙動変更時に人間向け文書と INDEX.md が更新されているか
- 構造検証: quality.toml の禁止依存、循環、層越え、複雑度予算に反しないか
- 生成検証: .owox/ から生成した target harness と実ファイルが一致するか
- 承認検証: 保護された正本変更に permissions.toml / CODEOWNERS 上の承認があるか

## 採用理由

- 旧 control harness の hook (context-index 等) が proven で、生成物へ昇格できる
- 機能別ファイルは関心ごとに読め、最小コンテキストと一貫する
- hook が owox 実行ファイルを呼ぶと、ロジックを core に集約でき、型付き正本の旗と一貫する
- 第1段階にスキル・ライフサイクルを入れると、検証つき成長を早く実証できる
- 正本ソースと生成物を分けると、再生成で正本が壊れない
- project.toml / change.toml / governance.toml を別正本にしないことで、brand / rules / permissions と二重管理にならない
- requirements/ と quality.toml は、task・verification・適応度関数の入力として機械検証価値が高い

## 捨てた案

- 設定を owox.toml 一つへ集約
- 純シェル hook
- hook を置かず MCP で全部やる
- 第1段階を最小土台だけにする
- project.toml を新設する
- change.toml を新設する
- governance.toml を新設する
- failures/ を新設する

## 捨てた理由

- 一つへ集約は部分読みしにくく、最小コンテキストと相性が悪い
- 純シェル hook はロジックが散り、検証・型保証が効かない
- hook 無しは CLI イベントで機械的に止める力が弱い
- 最小土台だけでは、検証つき成長の実証が後ろ倒しになる
- project.toml は brand.toml と目的・成功条件が重複する
- change.toml は rules.toml と変更方針・依存追加条件が重複する
- governance.toml は permissions.toml / CODEOWNERS と承認境界が重複する
- failures/ は verification / decisions / experience の link で表現でき、失敗記録だけ孤立しやすい

## 見直し条件

- .owox/ のファイルが増えすぎ、探しにくくなった時
- hook から呼ぶ owox サブコマンドが肥大化した時
- 第1段階のスキル・ライフサイクルが重く、初期検証が滞った時
- 生成物の種類が CLI 差分で大きくぶれた時
- quality.toml が rules.toml と二重管理になった時
- requirements/ が owlspec の領域を侵食し始めた時
