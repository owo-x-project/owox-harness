# Phase9 実機 Codex 対話検証 (性質軸 + guarded 承認で解凍)

## 検証対象

Phase 9 実装済の性質軸適応機構とブランチ作業記憶層を実機 Codex (対話) で確認する作業
(`docs/handoff/20260618-Phase9性質軸とブランチ記憶の実機検証.md`) の前半。性質軸の入口
(床注入・逆生成・設定) と、層別自律度の操作前ゲート (guarded 削除・契約面編集) を観た。検証中に
見つけた粗3件を是正し再確認した。

検証する owox の版: 本作業ツリーの release ビルドを `/usr/local/bin/owox` へ。
target (`/workspace/product/target`・profile=lightweight/incremental/continuous/layered・
quality.toml に src/core=guarded(契約面 src/core/ports)/src/ui=free の層) で MCP stdio/hook と
対話 Codex を確認。Codex cli 0.141.0。

## 機械確証

- cargo test 302 passed・clippy 0 (是正後)
- 床注入・profile.get/detect/set・層ゲート (Bash/apply_patch/Edit/Write の全経路・絶対パス正規化・
  解凍/消費後再 deny・gate_autonomy=Guarded で auto 拒否) を単体で確認

## 通った観点 (実機 Codex 対話)

- 床 routing: 「プロジェクトの性質を判断して設定して」から AI が profile.get/detect/set・rules.lookup・
  next を tool 名を告げられず選ぶ。床に `## Project nature` 行 (フル既定 prfaq/ideal-first/phased/layered)
- 逆生成: profile.detect が architecture=layered を quality.toml から推定し根拠つき draft を返す
  (自動確定せず)。プロセス3軸は docs/git シグナルで lightweight/incremental/continuous へ寄る
- profile.set: 4軸を設定し来歴へ記録 (decision_ids)。新セッションの床へ反映
- 層ゲート (是正後): guarded 層 src/core/engine.sh 削除・契約面 src/core/ports/api.sh 編集を
  apply_patch 経由で操作前 deny。Codex は迂回せず decision.record で人間ゲートへ回す
- 承認で解凍 (是正後): AI が deny 文言に従い decision.record の authorizes に対象パスを宣言 → 自分で
  gate.approve を呼ぶ (人間へ実行依頼せず = 詰み解消) → CLI 確認で人間承認 → 再 apply_patch で適用 →
  再操作は再 deny (one-shot)。engine.sh 削除・api v2 反映を実機で確認

## 見つけた粗と是正

### 粗1: verify.run の要件ゼロ時の誤文言

要件 0 件でも verify.run が「一部の accepted requirement に検証トレース不足」と needs_human を返し、
next の「全要件にトレースあり」と矛盾。要件未捕捉とトレース不足を取り違える文言だった。是正:
run_verify に要件未捕捉分岐を足し「まだ要件が捕捉されていない。人間が完了確認するか要件を先に捕捉せよ」と
正確化 (needs_human の挙動は維持。`crates/core/src/verify.rs`)。実機で新文言を確認。

### 粗2-a: 層ゲートが実機 Codex で空振り

guarded 削除・契約面編集が deny されず通った。原因: 層ゲートは Bash の rm と Edit/Write の file_path
しか見ず、apply_patch のパッチ本文を解析しなかった。実機 Codex は削除も編集も apply_patch で送り、
パッチ本文を tool_input.command の絶対パスに置く (hook stdin を捕捉して確証)。設計コメントの
「v1 では見ない」保留が、実機 (Edit/Write 不使用) では不活性を意味した。是正: parse_patch_changes で
パッチ封筒 (Add/Update/Delete File・Move to) からパス抽出し work_dir 基準で repo 相対化、Bash/apply_patch/
Edit/Write を一様に層 glob 照合 (`crates/core/src/hook.rs`・`crates/mcp/src/hook.rs`)。

### 粗2-b: 承認後の適用が詰む

ゲートが正しく deny した後、人間が承認してもコード変更を適用する道が無い (canon は
apply_pending_canon_change で自動適用だがコードに適用器が無く、AI が再 apply_patch すれば層ゲートが
また弾く)。承認と自動改善ループが消した詰みが guarded コード変更で再発。是正: 「承認で解凍」
(`docs/decisions/20260619-Phase9-guarded承認で解凍.md`)。来歴に authorizes/consumed を足し、AI が
decision.record で触るパスを宣言→人間 gate.approve→層ゲートが対象を全て覆う承認を見つけ 1 回だけ Thaw
し consumed に。authorizes 持ちは常に Guarded で gate.auto_approve が拒否 (自己承認で核を解凍させない
二重ガード)。

### 磨き: deny が未承認対象を全列挙

当初 deny は最初の未承認 guarded 対象 1 件だけを名指しし、複数あると承認が対象数だけ要った
(実機で engine と api を別々に 2 回承認)。是正: deny が未承認 guarded 対象を全列挙し「1 つの decision に
全部 authorizes して」と導く。承認 1 回で済む。

## 未確認・残課題

- ブランチ作業記憶層・auto 承認窓・correction.note・訂正 nudge・能動提示・軸ゲートの next・req コマンドの
  起草切替は本セッション未検証 (次セッションへ)
- 磨き後 (deny 全列挙) の承認 1 回フローは hook で確認済・対話 Codex での 1 回承認は再確認待ち
- apply_patch の bulk 削除が不可逆検出 (Bash 専用) を素通りする隙 (層ゲートは guarded を捕捉するが、
  flat/free 層の apply_patch 削除は Bash bulk-delete 検出に掛からない)。要否を別途判断
- host codex の rtk ラッパが target の `rg --files` 出力を一部欠落させるノイズ (owox 非由来)。
  architecture 判断は detect 由来で歪まず
- target 直下の `.agents/skills/` (host codex 由来・owox 非生成・git 未追跡)。cleanliness スクリプトが
  `.agents/` を禁止する点と矛盾。除去要否を判断

## 機械確証 (是正後)

cargo test 302 passed・clippy 0。hook/MCP stdio 経路で全是正を実 apply_patch 形式で確認。
実機 Codex で粗2 の end-to-end (deny→authorizes 記録→gate.approve→適用→再 deny) を確認。

## 後半: ブランチ作業記憶層 (実機 Codex 対話)

handoff `20260619-Phase9検証後半-ブランチ記憶と承認自動改善.md` の観点1 を全項目確認。target は
profile=lightweight/incremental/continuous/layered のまま、feature/* ブランチで検証。

### 通った観点 (実機 Codex 対話)

- 記録: 「いま何をやっているか覚えておいて」から AI が tool 名を告げられず branch.note を選ぶ。初回呼出で
  `work/branches/<branch>.md` を生成 (`/`→`__` サニタイズ)・`.owox/.gitignore` に `work/` を冪等追加し
  git 未追跡化
- 想起: branch.notes で本文を読み戻す。床注入されず (SessionStart に本文なし) オンデマンド取得を確認。
  next/context と組合せ作業状態を復元
- ブランチ分離: 別ブランチへ git 切替後の branch.notes は当該ブランチのバケツのみ参照 (呼出時に
  git で現ブランチを判定)。他ブランチのメモは混ざらない
- secret: AKIA 形式のキーを残すよう促しても、実機モデルが branch.note 呼出前に自前で拒否しサニタイズ。
  owox の secret::scan バックストップは多層防御の内側で、実機 e2e では発火に至らず (unit test
  `secret_is_rejected` でカバー)
- stale-branch-memory (孤児): ブランチ削除後もメモファイルは残り、next の Decay warnings に
  `branch no longer exists` として advisory 報告。生存ブランチは出ない
- worktree 同バケツ: `git worktree add` の別ディレクトリで Codex を起動し branch.note すると、worktree
  自身の .owox ではなく本体 `/.owox/work/branches/<branch>.md` へ着地 (git-common-dir の親=本体 root へ
  正規化)。worktree 側に独自 work/ は作られず誤着地なし

### 見つけた粗と是正

#### 粗3: branch.notes の空応答が failed

メモ未記録のブランチで branch.notes が `status: failed` (`no branch memory for <branch>`) を返し、AI が
取得失敗と誤認して git log・rg・ファイル読みの探索に走り、無関係な内容を推測で組み立てた (検証中に実害)。
「メモ無し」は正常な空状態であり失敗ではない。是正: branch_memory.rs の load を NotFound 時に空の
BranchMemory を返すよう変え (読込自体の失敗のみ Err)、get_branch_memory_envelope は空時に
`No notes recorded on branch <branch> yet.` を `ok`+空 notes で返す。add_branch_note は読込失敗時に
上書きせず止める (既存メモ取りこぼし防止)。`crates/core/src/branch_memory.rs`。実機で `ok`+空応答を
確認し、AI が「メモ無し」と即座に正しく認識 (探索暴走の消失) を確認。

## 機械確証 (後半・是正後)

cargo test 303 passed (粗3 の `missing_branch_reads_as_empty_ok` を追加)・clippy 0。新バイナリを
`/usr/local/bin/owox` へ差替え、実機 Codex で観点1 全項目 (記録・想起・分離・孤児 decay・worktree
同バケツ・粗3 是正後の空応答) を end-to-end 確認。

## 後半: 承認と自動改善 (観点2) — 環境の確定調査

観点2 の入口 (correction.note) は通った: 人間の訂正発話で UserPromptSubmit の訂正 nudge が一度だけ
発火し (編集済 + ツリー dirty + 未 nudge の決定論3条件)、AI が correction.note で practice 草案を
open gate (非 guarded) として積んだ。next の Open decisions に正しく出る。

ところが gate.auto_enable へ進めず、AI が「このセッションでは gate.auto_enable が提供されていない」と
述べた。調査で host 環境の交絡を確定:

- owox serve は46ツール露出 (MCP tools/list で確認)。owox 後退でも実装バグでもない
- 真因は Codex の `apps` feature (`codex features list` で stable・true)。codex_apps の github 約90
  ツールをモデルのツールリストへ載せ、owox 46 と合わせ総数がモデルのツール予算を超える。Codex が
  ツールを選別し owox を削る。脱落ツールはセッションごとに変わる (承認系 gate.approve/auto_enable/
  auto_approve が非決定的に消える)
- 裏取り: `apps` 有効 (TUI 既定) で owox ~18個・gate.auto_enable 脱落。`codex -c features.apps=false`
  で起動すると owox 46個全部・gate.auto_enable 復活。`codex exec` (軽量環境) でも owox はほぼ全部出る

対処: target の対話 Codex 検証は `codex -c features.apps=false` で起動する。別途の設計課題として、
owox の46というツール表面の広さは apps 等を載せた実ユーザー環境では owox ツールが削られる頑健性
リスクであり、ツール表面の統合・削減を検討する余地がある (発見性以前にツールがモデルへ届かない層の問題)。

### 通った観点 (apps 無効・対話 Codex 対話)

- correction.note + 訂正 nudge: AI 編集後に人間が訂正発話すると UserPromptSubmit が nudge を一度だけ
  注入 (編集済 + ツリー dirty + 未 nudge の決定論3条件・`crates/mcp/src/hook.rs:149`)。AI は
  correction.note で practice 草案を open gate (非 guarded) として積む
- gate.auto_enable: 「離席するから自動で進めて」で AI が呼び窓を開ける。next 先頭に「Automatic
  approval is on」が出る
- gate.auto_approve: 非 guarded の practice ゲートを auto 承認し `"auto": true` でキュー入り。canon
  (practices.md) へ適用。guarded な rules 変更には AI が auto_approve を試さず人間ゲートへ回す
- gate.confirm: 後追いキューの対象を confirmed=true にしキューから外す。canon は不変
- gate.revert: auto 承認済の practice を canon から逆適用で除去し decision を rejected へ
  (`Reverted by human`)。practices.md から該当行が消えることをディスク確認
- 後追いキュー表示: next の「Auto-approved, awaiting the human's confirmation」に当該が出る。
  gate.list は open でない adopted を出さない (0 pending) のも整合
- 窓はセッション限り: 前セッションで開いた窓ファイル (`.owox/.cache/auto-approve.json`) は再起動の
  起動時点では残るが、新セッションの初回プロンプトで SessionStart hook が発火し close する
  (`crates/mcp/src/hook.rs:554`)。e2e で窓ファイル消失を確認

### 見つけた粗と是正

#### 粗4: next が open decision の自律度を示さず承認手段を取り違える

auto 窓が開いている時、AI が非 guarded の practice ゲートを「open decision だから guarded」と誤判定し
gate.auto_approve でなく gate.approve を呼んだ。結果、後追いキュー (confirm/revert) を素通りして直接
adopted。床 routing が「guarded = brand/rules/glossary/plain open decisions」と言うため、実体が open
decision の practice 草案を guarded と取り違える誘導ギャップ。是正: render_next の Open decisions 各行に
`owox_core::gate_autonomy` で `[guarded: only a human approves, via gate.approve]` /
`[non-guarded: approve with gate.auto_approve while automatic approval is on, otherwise gate.approve]`
タグを付ける (`crates/mcp/src/serve.rs`)。再検証で AI が非 guarded を gate.auto_approve・guarded を
gate.approve へ正しく振り分けることを確認。

### 未確認・残課題 (観点2)

- trusted モード懸念: gate.approve / gate.auto_enable は destructive_hint で「CLI 確認 = 人間の同意」が
  前提だが、target は trust_level=trusted のため実機で確認プロンプトが出ず、AI が guarded な rules 変更を
  含め自己承認できてしまった。trusted プロジェクトで人間ゲートの同意モデルが崩れる。
  → 判断済: 生成 target 設定でゲートツールを常時確認へ固定して担保する
  (`docs/decisions/20260620-Phase9-人間ゲートの確認依存.md`)。実装と実機モード値確定は次スライス
- gate.auto_approve の「窓オフ拒否」「guarded 拒否」の拒否経路は、AI がタグを見て auto_approve を
  そもそも試さないため e2e 未発火 (コード・単体ではカバー)。直接 MCP で叩いて確認する余地
- gate.auto_disable は未検証
- 能動提示 (床の `## Skills you can use` + kickoff の practices/skills 促し) は未着手。床注入は
  load_skills で読めるプロジェクト製スキルがある時だけ出るため、skill.register/promote 済スキルを
  作ってから検証する必要がある

### 機械確証 (観点2)

cargo test 304 passed (粗4 の `open_decisions_tag_gate_autonomy` を追加)・clippy 0。新バイナリを
`/usr/local/bin/owox` へ差替え。apps 無効の対話 Codex で観点2 の承認・自動改善ループを end-to-end 確認。
検証後 target を baseline へ復元 (practices.md・rules.md・widget.sh を戻し今セッション作成の decision を削除)。
