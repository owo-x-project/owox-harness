# Phase6 スライス3: quality.toml の適応度関数の設計

## 状態

採用 (実装前)。Phase6 スライス3 (`docs/requirements/20260614-Phase6-検証を閉じる.md`)。
品質バーを quality.toml で宣言し、言語非依存で機械検証する (`docs/decisions/20260611-品質保証.md`)。

## 背景

品質保証の決定で、品質バー・依存方向・層境界・循環禁止・複雑度予算を quality.toml に置き機械検証すると決めた。owox は言語非依存で深い構文解析を持たない。検証できる範囲を絞り、深い解析は config.toml の検査コマンドへ委譲する。

人間判断 (plan の論点) を確認し確定した:

- owox 直接検証の範囲 = ファイル行数予算 + 禁止パターン (層境界/依存方向)
- 違反の効かせ方 = commit ゲートで phase 適応 (保守=block・初期/安定=警告)、verify.run は常時報告

## 採用設計

### quality.toml は別ファイルの machine 設定

config.toml (生成・検証設定) と別に quality.toml (品質制約) を置く。「rules=いつ止めるか、quality=何を守るか」の役割分担 (`docs/decisions/20260611-品質保証.md`)。TOML (入れ子のある machine 設定)。model.rs に Quality 型、load.rs で任意読込 (brand 以外は任意の方針)。Canon へ足す。無ければ無効 (opt-in)。

### owox 直接検証は2種 (言語非依存)

ファイルを見るだけで分かるものに限る。構文解析は持たない。

- ファイル行数予算: `[[quality.budgets]]` = paths (glob) + max_lines。glob に当たるファイルの行数が上限超なら違反
- 禁止パターン (層境界/依存方向): `[[quality.boundaries]]` = paths (glob) + forbid (正規表現の並び) + reason (任意)。paths 配下のファイルに forbid のいずれかが現れたら違反。「domain は infra を参照禁止」をプロジェクトが正規表現で表す (rules の detect: と同じ発想で、owox は意味を解さず文字列照合する)

検査コマンドへ委譲 (owox は持たない): 循環依存・複雑度など、依存グラフや構文解析が要るもの。config.toml の `[[verify.checks]]` にコマンドとして書く (既存機構)。

### glob はファイル列挙を呼び出し側が与え、照合は正規表現へ変換

- 新規依存を足さない。glob は既存の regex クレートへ変換して照合する (`**`→任意の深さ、`*`→区切り内の任意)
- 検証対象ファイルの列挙は mcp が `git ls-files` で行い (build 生成物・無視ファイルを避け速い)、失敗時は単純な走査へ退避する。core の run_quality は work_dir と相対パスの並びを受け、各ファイルを読んで判定する (core は git/走査を持たず決定論・テスト容易。today/known_checks と同じく外から与える方針)

### run_quality は違反の並びを返す

core `quality.rs` に `run_quality(quality, work_dir, files) -> Vec<QualityViolation>`。
QualityViolation = 種別 (budget/boundary) + path + detail (超過行数・当たった禁止パターン)。

### 効かせ方: commit は phase 適応・verify.run は報告

- commit ゲート: quality 違反を未承認 gate と同じ手口で phase 適応する。保守 (maintenance) は block、初期/安定は警告。これが「弾く」歯。コアゲート (test/build) は phase 不問で常時維持、quality はコアゲートでないため state 適応 (`docs/decisions/20260611-品質保証.md`)
- verify.run: quality 違反を常に data の quality へ報告 (助言)。封筒 status は変えない (完了3区別の verification は config 検査のまま。quality を verification 完了へ混ぜない)。判定は commit の1か所へ集約し重複させない
- これで「初期=緩い」を保ちつつ、保守で違反を機械で弾く。違反は phase 不問でいつでも verify.run に見える

## 変更境界

触るもの:

- crates/core/src/model.rs: Quality 型と quality.toml 読込、Canon へ追加
- crates/core/src/load.rs: quality.toml の任意読込
- crates/core/src/quality.rs (新規): run_quality と glob→regex・行数/パターン判定
- crates/core/src/verify.rs: verify.run が quality 違反を data へ報告
- crates/core/src/hook.rs: commit_gate に quality 違反の phase 適応を足す
- crates/mcp/src/serve.rs・hook.rs: ファイル列挙 (git ls-files) を渡す配線
- crates/core/src/lib.rs、docs

触らないもの:

- requirements 正本層・要件完了の機械判定 (スライス1・2 で確定)
- 多視点レビュー (スライス4)
- 完了3区別の verification 完了の意味 (config 検査のまま)
- stop ゲート (quality は commit と verify.run に載せる)

守る振る舞い:

- core は git/走査を持たず決定論 (ファイル列挙は mcp が与える)
- quality.toml 無しなら従来どおり (opt-in)
- コアゲート (検査失敗) は phase 不問で常時 deny

## 危険

- glob→regex 変換の取りこぼし (`**` と `*` の境界)。最小の構文に絞り単体で固める
- 大きな repo のファイル走査コスト。git ls-files で tracked のみに絞り build 生成物を避ける。git 無し時の退避走査は `.git`/`.owox` を除外する
- 禁止パターンの正規表現はプロジェクト記述。読込時に妥当性を検証する (irreversible の detect: と同じく誤記を早期に弾く)
- 違反の効かせ方を commit に集約するため、初期 phase では verify.run の報告だけで block しない。「弾く」確認は保守 phase で行う

## 検証方針

- 単体: glob→regex 変換、行数予算違反/適合、禁止パターン違反/適合、正規表現の妥当性検証、quality.toml 無しは空、commit_gate の phase 適応 (保守 block・初期警告)
- 結合: verify.run が data.quality に違反を出す、commit ゲートが保守で quality 違反を deny
- 手動 (後の一括対話検証へ): quality.toml を書き、層境界違反・行数超過を仕込み、保守 phase で commit が弾くか、verify.run が違反を示すか
- 未確認として残す: 差分量予算 (将来)・per-rule severity (常時 block する規則の指定。将来)

## 捨てた案

- quality を verification 完了へ混ぜる (違反=検証失敗) → 完了3区別の verification (config 検査) の意味が濁る。別軸で報告する
- 判定を verify.run と commit の両方に持つ → phase ロジックが二重化。歯は commit に集約し verify.run は報告に徹する
- owox が import 解析・循環検出を内蔵 → 言語依存で対象外 (汎用道具化) に近づく。検査コマンドへ委譲
- glob ライブラリを新規依存で足す → regex 変換で足りる

## 見直し条件

- 禁止パターンの文字列照合では層境界の表現に足りないと分かった時
- ファイル走査が重く .gitignore 連動や対象限定が要る時
- quality 違反に per-rule severity (初期でも常時 block する規則) が要る時
- 差分量予算を commit 文脈で足す判断がついた時
