# Phase9 追補: 経験層スケール・GitHub 連携・kickoff 束ね

人間の4要望を確定し採用判断を残す。4件は独立だが同一セッションの要望のため1文書へ束ねる。

## 状態

採用判断・実装対象。実装は本文書の設計に従う。真実源は実装と本文書。

## 要望の出所

人間の4発言:
- glossary / practices が肥大化した時の対策・オンデマンド化
- glossary に別名を足せるように (数は制限)
- task 管理の GitHub Issues との同期・取り込み
- kickoff・既存プロジェクトからの再生成

## 1. 経験層スケール (肥大化対策・オンデマンド化)

### 現状

- glossary は用語名のみ床注入・定義は出現時 push (既にオンデマンド)。肥大化源は用語名一覧。
- practices は本文全数を床へ常時注入。最大の肥大化源。
- decay (古い指針の見直し合図)・redundancy (類似指針の統合促し) は advisory で既存。

### 採用案: 件数閾値で自動降格

床に出す件数の上限を config.toml の `[context]` で持つ (machine 設定。Settings へ取り込む)。既定は十分大きく、小規模では現状どおり全注入し発見性を最優先する。上限超過時だけ床を縮める。

- practices: 上限超過なら床へは新しい順 (鮮度高い順) に上位 N 件だけ出し、末尾へ「古い指針は practice.lookup で引ける」の1行を添える。残りは practice.lookup で語句検索のオンデマンド取得。
- glossary 用語名: 上限超過なら床から用語名一覧を外し「用語が多数ある・定義は出現時に届く・glossary.lookup で引ける」の1行に置換。定義の出現時 push は全 canon から効くため発見性は push と routing が担保する。

### 理由

- 小規模では発見性最優先で全注入、大規模で初めてトークンを守る。固定閾値で振る舞いが切り替わり清潔。
- 発見性は床 routing が駆動する (実機教訓)。降格時は routing 行へ lookup を明示し発見性の穴を塞ぐ。
- 鮮度高い順は decay と整合 (新しい指針ほど効く)。

### 捨てた案

- 即オンデマンド化: 小規模でも発見性が落ちる。
- decay 強化のみ: 床注入量を直接は下げず根治しない。

### 新設

- practice.lookup tool + core 関数 (practices.rs)。語句の部分一致で指針を引く。読みは別 tool の既存流儀 (glossary.lookup / knowledge.lookup) に揃える。
- Settings へ床上限 (practices_floor_max・glossary_floor_max)。config.toml `[context]`。

## 2. glossary 別名 (数制限)

### 採用案

GlossaryEntry へ aliases を足し、正本書式はパイプ区切り `- 用語 | 別名1 | 別名2: 定義`。

- 照合 (glossary_injection・glossary.lookup・canon.propose の用語照合) を用語 ∪ 別名へ広げる。注入・dedup の鍵は正規の用語名へ寄せる。
- 別名は床へ出さない (床は正規用語名のみ・肥大化を増やさない)。別名は出現時照合専用。
- 数制限: 1 用語あたり別名の上限を定数で持ち、glossary.add が超過を弾く。正本の人間手編集は固定層として弾かず受ける (load は寛容)。

### 理由

- パイプは括弧補足を避ける方針と整合し機械パースも堅牢。コロンは定義の区切りなので別名側 (コロン左) に限りパイプで分ける。
- 別名を床に出さないことが肥大化対策と整合。

### 捨てた案

- 括弧表記: 注入文の括弧禁止方針と不整合。
- 別名を床へ出す: 肥大化を増やす。

## 3. task の GitHub Issues 同期・取り込み

### 採用案: skill 層で双方向・external link で対応付け

owox core はネットワーク非保持を維持する。GitHub 連携は AI が gh CLI を Bash で呼ぶ命令型の入口 skill で実現し、core は対応付けの保管だけを担う。

- task へ external link を追加 (`## External` の `- github: owner/repo#123`)。再同期で重複作成しないための対応付けを永続する。task.create / task.update が external を受ける。
- 入口 skill (commands の標準コマンド) `issues` を足す。本文は双方向の作法を命令形で示す: 取り込み = gh で issue を引き未対応の issue を task.create + external 記録、同期 = external 未設定の open task を gh で issue 化し external を記録。
- owox が正本。GitHub は写し。衝突時は owox を優先する旨を skill 本文へ明記。

### 理由

- core を純粋に保ち認証・秘密・依存・ネットワークを抱えない (これまでの設計の清潔さを維持)。
- 「AI に技を持たせる」owox 思想と整合。external link が双方向の対応付けを単一に持つ。

### 捨てた案

- core にネットワーク連携: 認証・秘密・依存が増え core を汚す。
- 片方向のみ: 人間要望は同期+取り込みの双方向。

### 見直し条件

- gh 不在環境での失敗の扱い・external 形式の他 forge 拡張・standard か opt-in か (当面 standard・gh が無ければ skill は無害)。

## 4. kickoff 束ね・既存コードへの後入れ

### 現状

部品は揃う: kickoff は散文 skill が context→next→profile.detect→canon.detect→propose→profile.set/canon.add を順に呼ばせる。再生成は generate / setup が MergeJson/MergeToml で人間設定を壊さず作り直す。

### 採用案: kickoff tool に detect 群を束ね、後入れを能動化

kickoff を MCP tool 化し、立ち上げに要る読取と逆生成を1呼び出しで束ねて返す。

- 返すもの: Vision・phase・nature (未設定なら未設定の明示)・next 要約。nature 未設定なら profile.detect の draft + 根拠。canon が手薄な既存コード (層宣言なし等) なら canon.detect の draft + 根拠。
- 全て draft / 提案で、書かない (profile.set / canon.add は人間確認後)。
- kickoff skill 本文は「kickoff tool を呼ぶ」へ寄せ、散文の手順列挙を tool 内へ畳む (入口 skill を 1 tool へ強束ねる既存方針と整合)。
- 後入れ導線: kickoff tool が「正本が手薄な既存コード」を検知し detect draft を能動返却する。これが既存コードへの後入れの一筆書き入口。

### 理由

- 散文任せの一括性・確実性の弱さを tool 内へ畳んで底上げ (req 是正と同じ教訓: 床ルーティングが散文へ流すと本文が読まれない)。
- detect 部品の再利用。新しい逆生成ロジックは足さず束ねるだけ。

### 捨てた案

- 現状維持 (skill 文面磨きのみ): 一括性・確実性が上がらない。
- 再生成専用導線の新設: generate / setup の既存導線で足り、二重化は不要。

### 見直し条件

- 後入れ検知のヒューリスティック精度・kickoff tool の返却が大きすぎる場合の分割。

## 実機検証 (2026-06-21 / Codex gpt-5.4-mini)

4機能を target repo の実機 Codex で対話確認。全合格。手順・所見は docs/handoff/20260621-経験層スケール他4機能の対話検証.md。

### 確認できたこと

- 経験層スケール: 床降格 (glossary 件数1文・practices 新しい順N件+lookup案内)、practice.lookup の空クエリ全件新しい順・語句絞り込み・床外の古い指針取得。
- glossary 別名: 別名→正規用語の解決 (lookup・UserPromptSubmit push)、別名は床/push へ非露出、canon.add の別名衝突・上限超過の弾き。
- GitHub 双方向: task external の往復・重複防止、issues skill 本文の遂行、gh 不在の graceful 報告。
- kickoff 束ね: 1呼びで Vision・phase・nature・next、宣言済は draft 無し、未宣言は detected_draft+profile.set 案内、手薄ガードレール+既存コードで guardrails_draft 相乗り。thin_guardrails は layers・boundaries・irreversible の3条件すべて空が要件 (部分ガードレールでは誤発火せず)。kickoff skill は tool→next→context の順で立ち上げ。

### 見直し条件 (実機で見えた粗)

- skill 発見性がモデル依存: 弱いモデルは plain text の skill 名指しで登録済 skill 一覧を無視しファイルを漁る。`$名` の強名指しなら従う。skill 登録自体は正常 (`.agents/skills/<id>/SKILL.md` がセッション一覧へ locator 付きで提示)。owox 生成は変えず Codex 側の限界として据え置き。床肥大化を避ける方針と整合。強いモデルでの再確認を残す。
- 日付なし practice 行はパースで脱落 (床・件数・lookup に出ない)。canon.add は常に日付付与だが手編集の日付なし行が消える。許容か明示弾きかを要判断。
- guardrails_draft のレイヤ path が `/ports/`・`/core/` 形式で glob でない。canon.detect の path 推定が貼り付け用 quality.toml スニペットへそのまま乗ると実ファイルに当たりにくい。
- issues skill 本文に gh 失敗時の挙動の明示なし。実機は破綻しなかったため優先度低。詰まれば1文追加。
