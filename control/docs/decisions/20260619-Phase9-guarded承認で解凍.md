# Phase9 guarded 承認で解凍

採用判断。2026-06-19。実機 Codex 対話検証で見つけた粗の是正。
元: `docs/decisions/20260618-Phase9-性質軸適応機構.md` (guarded 人間ゲート)・`docs/handoff/20260618-Phase9性質軸とブランチ記憶の実機検証.md`。

## 背景

実機 Codex 検証で、guarded 層の操作前ゲート (削除・契約面編集) が2つの粗を抱えていた。

- 粗1 空振り: 層ゲートは Bash の rm コマンドと Edit/Write の file_path しか見ず、apply_patch のパッチ本文を解析しなかった。実機 Codex は削除も編集も apply_patch で送りパスを本文 (tool_input.command) に置く絶対パスのため、層ゲートが完全に不活性だった。設計コメントは「v1 では見ない」と保留していたが、実機は Edit/Write を使わないので保留=不活性を意味した
- 粗2 適用の詰み: ゲートが正しく deny した後、人間が承認しても適用する道が無い。canon は apply_pending_canon_change で承認時自動適用されるが、コード変更には適用器が無い。AI は decision.record で open 判断を残すが、それは適用すべき中身を持たず、再度 apply_patch すれば層ゲートがまた弾く。承認と自動改善ループが消したはずの「詰み」が guarded コード変更で再発していた

## 採用した案

### 粗1: 層ゲートが全経路の操作対象を見る

`layer_pre_action_gate` を、操作対象パスを tool 経路ごとに集める形へ拡張した。

- Bash: rm / git rm のコマンドから (従来通り)
- Edit / Write: file_path から (従来通り)
- apply_patch: パッチ封筒 (`*** Add/Update/Delete File:`・`*** Move to:`) を解析して (新設 parse_patch_changes)
- パスは絶対なので work_dir 基準で repo 相対へ正規化してから層 glob へ照合する

これで Codex がどの経路で編集・削除しても guarded 削除・契約面編集を捕捉する。

### 粗2: 承認で解凍 (来歴を単一の真実源にする)

guarded 人間ゲートの「承認時に適用」を、canon.propose の仕組みに対称な形でコード層へ広げた。

- 来歴 (Decision) に `authorizes` (承認時に解凍する具体パス) と `consumed` (使い切り印) を足す。proposed_change が canon 適用を担うのと対称に、authorizes はコード操作を担う
- AI は層ゲートで止まると decision.record で触りたいパスを authorizes に宣言し open ゲートを上げる。入口は decision.record を拡張 (専用 tool を増やさない)
- 人間が gate.approve すると adopted になる。層の操作前ゲートは、guarded 操作対象が adopted・未消費の authorizes に全て覆われていれば 1 回だけ通す (Thaw) ・その来歴を consumed にする。1 つでも未承認なら従来どおり deny
- 同じパスを再び触るには再ゲート (one-shot 再凍結)

採用理由:

- 来歴が単一の真実源。承認の証跡と解凍権限が同じ来歴に乗り、cache に並行状態を作らない。authorizes は .md に描画され人間が gate.approve 時に対象を目視できる
- gate_autonomy は authorizes 持ちを常に Guarded と判定するので gate.auto_approve が自動拒否する。AI が自己承認 (auto) で核を解凍できない二重ガードが効く
- proposed_change (canon) と authorizes (code) の対称で、guarded ゲートの承認時適用を両系統で揃う

不採用: cache へ解凍トークンを書く案。来歴と二重管理になり承認証跡が散る・再起動/compact での寿命が曖昧で清潔さに劣る。

### 責務分割

依存方向 mcp→core を保つ。`layer_pre_action_gate` は純ロジックで、承認済み解凍ゲート一覧を引数で受け Allow / Deny / Thaw を返す。状態 (来歴の収集・消費) は mcp が担う (来歴 IO は元々 mcp 側)。

## 実装

- core record.rs: Decision に authorizes/consumed・render/parse・record_decision_with_authorization・mark_gate_consumed・gate_autonomy が authorizes 持ちを Guarded 保証
- core hook.rs: GateAuthorization・LayerGate・layer_pre_action_gate 拡張・parse_patch_changes・relativize_path
- mcp hook.rs: adopted 未消費 authorizes 来歴の収集・Thaw で消費して通す配線
- mcp serve.rs: decision.record に authorizes 入力

## 確認

- 機械: cargo 301 passed・clippy 0 (パッチ解析・絶対パス正規化・解凍/消費後再 deny・gate_autonomy=Guarded・auto 拒否)
- hook/MCP 経路: 実 apply_patch 形式で guarded 削除/契約面編集 deny・承認後 Thaw・consumed・再 deny を確認
- 実機 Codex end-to-end: deny→authorizes 付き記録→gate.approve→再 apply_patch で適用→再実行 deny は実機検証で確認する

## 見直し条件

- AI が authorizes を正しく埋めない頻度が高い時 (deny 文言・床の誘導を調整)
- 解凍をパス単位でなく操作種 (削除/編集) でも絞る要が出た時 (現 v1 はパス一致のみ)
- apply_patch の bulk 削除が不可逆検出 (Bash 専用) を素通りする隙の是正要否
