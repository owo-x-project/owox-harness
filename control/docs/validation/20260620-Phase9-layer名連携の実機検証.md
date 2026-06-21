# Phase9 実機 Codex 対話検証 (layer タグと quality.toml 層名の連携)

## 検証対象

Phase 9 性質軸適応機構の未決 #5 (layer タグの値域) を実装で決着させた分の実機確認。
要件/タスクの layer タグを quality.toml `[[layers]]` の任意 `name` へ照合する機構
(`docs/decisions/20260618-Phase9-性質軸適応機構.md`・`crates/core/src/quality.rs` の
check_known_layer / layer_names) と、`next` の層別充足報告 (層名ごとのトレース数) を観た。

検証する owox の版: 本作業ツリーの release ビルドを `/usr/local/bin/owox` へ。
target (`/workspace/product/target`・profile=architecture=layered・他3軸はフル既定
prfaq/ideal-first/phased) で MCP stdio/hook と対話 Codex を確認。
quality.toml の層は src/core=guarded(name=core・契約面 src/core/ports)/src/ui=free(name=ui)。
src/app は実在するが層名未宣言。

## 機械確証

- cargo test 315 passed・clippy 0
- quality.toml の layer name パース・layer_names 集約・check_known_layer (空 layer 許容・
  宣言済許容・未知は有効名添えて弾く・name 未宣言なら照合せず) を単体で確認
- create/update_requirement・create/update_task が known_layers で未知 layer を弾くことを単体で確認

## 通った観点 (実機 Codex 対話)

### 弾く側 (未知 layer の拒否と AI の回復)

「core/ui/app の3改修をタスク登録し層タグを付けて」で:

- core→layer=core・ui→layer=ui は ok
- app→layer=app は failed「Unknown layer: app. Use a layer declared in quality.toml
  (declared: core, ui) or add it to [[layers]] first.」= 設計通り有効名を添えて拒否
- AI は黙って捏造せず context で状況確認 → 層タグを外して再登録 → 人間へ「app は未定義層」と
  日本語で報告 → 「app 層を追加する案を出す」と次手を提示。詰まらず自然に回復

### 報告側 (層別充足報告)

「core/ui の完成像を層タグ付き要件で2件登録」→ 両方 ok (layer=core・layer=ui)。
「次に決めること・着手タスク・要件の層ごとの進み具合を見せて」で AI が `next` を呼び:

```
要件の層ごとの進み具合
- unlayered: 0/4 requirements traced
- core: 0/1 requirements traced
- ui: 0/1 requirements traced
```

= `## Layer progress` が quality.toml の宣言層名 (core/ui) ごとに並び、層タグ無し要件は
unlayered へ集約・件数も正確 (新規 core/ui 各1・既存無タグ4)。層別報告とゲート層が同じ層名で揃う。

### 副次観察

- requirements-shape=prfaq が効き、AI が statement を Q/A 形式 (何を保証/何をしない) で起草
- 既存要件4件が無タグで unlayered バケツへ。正常 (任意タグ)

## 観察 (不具合ではない)

- src/app は実在しコードもあるが quality.toml で層未宣言 → どの層にも当たらず autonomy=Free で
  ゲート外。これは target の quality.toml が層を網羅していないだけで #5 の不具合ではない。
  むしろ #5 がその穴を可視化し、AI も「app 層追加案」で穴を指摘できた

## 結果

#5 (layer タグの値域) を弾く側・報告側とも実機 Codex で確認。不具合なし。設計の
「quality.toml を唯一の層真実として再利用し、層別報告とゲート層を同じ層名で揃える」が成立。

## 未確認事項

- target repo の清潔さ確認 (複数回検証の蓄積で .owox/decisions 等に未コミット差分。scripts/check-target-cleanliness.sh で次に確認)
- stage タグ (delivery=phased の段グルーピング) の実機確認は未 (今回は layer のみ)
