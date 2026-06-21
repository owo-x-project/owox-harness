# Phase6 スライス1: requirements 正本層の設計

## 状態

採用 (実装前)。Phase6 スライス1 (`docs/requirements/20260614-Phase6-検証を閉じる.md`) の設計。
記録層 (decisions・tasks) を手本に最小で立てる。

## 背景

要件・受け入れ基準・検証 link を持つ正本層を作る。読み書きはすべて tool 経由 (canon 直読み禁止)。
記録層 (`crates/core/src/record.rs`・`task.rs`) が1件1ファイル Markdown・ID=日付+slug・封筒返りの完成形なので、これを踏襲し新サブシステムを作らない。

要件の状態モデルは plan で提案済み: ライフサイクル状態 (draft / accepted / superseded) のみ持ち、充足 (met) は保存せず verify.run が受け入れ基準の検証 link から導出する (スライス2)。

## 採用設計

### 新モジュール requirement.rs

`crates/core/src/requirement.rs`。`.owox/requirements/<id>.md` に1要件1ファイル。
ID=日付+slug。record.rs の slugify・allocate_id を再利用する (crate 内 pub(crate))。

### 型

- RequirementStatus: Draft / Accepted / Superseded。人間が宣言するライフサイクル。充足は保存しない
- AcceptanceCriterion: id (永続番号) / title / given / when / then / verify (任意の検証 link)
- RequirementLinks: decision (関連来歴)
- Requirement: id / title / status / statement (要件本文) / criteria (受け入れ基準の並び) / links / supersedes (置き換えた旧要件 ID の並び)

### Markdown 符号化 (新文法を足さない)

要件ファイルの節:

- 1行目 `# <title>`
- `## Status`: draft / accepted / superseded
- `## Statement`: 要件本文 (散文)
- `## Acceptance criteria`: 受け入れ基準。rules の不可逆操作と同じ「エントリ + 属性行」方式
- `## Links`: `- decision: <id>`
- `## Supersedes`: 旧要件 ID の箇条書き

受け入れ基準は1基準が `- <id>: <短い名前>` のエントリ行、配下の `given:` / `when:` / `then:` / `verify:` が属性行。
これは rules.rs の parse_irreversible (エントリ + `detect:` 属性) と同じ読み方で、既存パーサ (`section.lines()` と split_pair) で複数基準・基準ごとの given/when/then/verify を読める。

例:

```
## Acceptance criteria

- 1: ログアウト時はログインへ誘導
given: 利用者がログアウト状態
when: ダッシュボードを開く
then: ログイン画面へ遷移する
verify: test_redirect_when_logged_out
- 2: 認証済みは閲覧できる
given: 利用者が認証済み
when: ダッシュボードを開く
then: 一覧が表示される
```

属性値は1行に保つ。長い背景は statement へ書き、基準は簡潔な given/when/then にする。

### 受け入れ基準の id は永続番号

基準追加時に既存 id の最大+1 を採番し、再利用しない。並べ替え・削除に強く、スライス2 の trace 欠落検出 (「要件 X の基準 2 に verify 無し」) が安定する。検証 link は基準単位で張る (plan の決定)。

### tool (6個。tasks と並ぶ surface)

- requirement.create { title, statement, status?, acceptance?[{given,when,then,verify?}] } → { id }
- requirement.list { status? } → { requirements:[{id,title,status,criteria,unlinked}] } (unlinked = 検証 link が欠ける基準数。スライス2 trace の素地)
- requirement.get { id } → 要件全文 (canon 直読み禁止の読み口。描画した本文を返す)
- requirement.update { id, title?, statement?, status?, links? } → { id }
- requirement.add_criterion { id, given, when, then } → { criterion } (次番号を採番)
- requirement.link_verification { id, criterion, verification } → 既存基準に検証 link を張る

create は受け入れ基準をまとめて受けられる (要件と基準を1呼び出しで書く一般形)。後から足す時は add_criterion、検証を書いてから張る時は link_verification。基準の操作 (追加・link) は専用 tool に分け、update は title/statement/status/links のみ扱う。update が基準をまとめて置換して取りこぼす事故を避ける。

### 来歴連動の線引き

- title / statement の変更 → reason 必須・adopted 来歴を残し requirement へ decision link。要件本文の本質変更は将来作業が黙って覆してはいけない判断そのもの (task.title が来歴連動の前例。要件はより強く該当)
- status 遷移・links 差し替え・基準の追加と link → 軽量 (来歴なし)。過剰記録へ逆戻りしない (`docs/handoff/20260613-Phase4対話検証で見つけた粗の改善.md` の方針)

### 充足は保存しない・next は触らない

要件ファイルに met フィールドを持たない。充足は verify.run がスライス2 で criteria の検証 link から導出する。
next tool はスライス1 では触らない。accepted かつ未充足の要件の可視化は充足導出が要るためスライス2。

## 変更境界

触るもの:

- crates/core/src/requirement.rs (新規)、lib.rs (型・関数の pub use)
- crates/mcp/src/serve.rs (6 tool と Params 型、get_info instructions に requirement 系を追記して tool 発見性を確保)
- tests/fixtures (必要なら要件サンプル)、docs (本設計・validation)

触らないもの:

- verify.rs (要件完了の機械判定はスライス2)
- quality (スライス3)、多視点レビュー (スライス4)
- next の描画 (スライス2)、hook
- targets/codex の生成ロジック (requirements は記録層で生成物でない。AGENTS.md には出さない)

守る振る舞い:

- 既存 tool・封筒・記録層の冪等性
- canon 直読み禁止 (読みは requirement.get・list 経由)
- core は時計を読まない (today は mcp が供給)

## 危険

- 受け入れ基準の given/when/then を複数行で書くと属性行の改行で壊れる → 属性値は1行とし長文は statement へ、と設計で回避する
- criterion id 採番はファイル内の最大値依存。要件編集は低頻度で1ファイル方式に従うため並行衝突は記録層全体と同程度の許容
- title/statement 変更の来歴連動は実機で過剰に感じないか確認する (Phase4 の過剰記録の教訓)

## 検証方針

- 単体: create→list→get の round-trip、criterion 採番、link_verification、title 変更の reason 必須・来歴連動、status 遷移、未知見出し・未知キーの reject、status の不正値 reject
- 結合: serve 経由の tool 呼び出し
- 手動: target サンドボックスで実機 Codex が requirement.create → add_criterion → link_verification → get を迷わず回せるか、requirements を直読みせず tool で読むか
- 未確認として残す: 充足の機械判定・trace 欠落検出 (スライス2)

## 捨てた案

- 受け入れ基準を `## Criterion: <名前>` の独立節で持つ → 予約節 (Status 等) と区別する分岐が増える。エントリ + 属性方式 (irreversible 流用) の方が既存資産に沿う
- update が受け入れ基準をまとめて置換 → 取りこぼし事故。追加・link を専用 tool に分ける
- 要件に充足 (met) を保存 → 編集後に古びる罠 (owox://state・verification を却下したのと同じ)。verify.run の導出に寄せる
- criterion id を並び順インデックスにする → 並べ替え・削除で id がずれ trace が壊れる。永続番号にする

## 見直し条件

- 受け入れ基準が長文化し1行属性で窮屈になった時 (符号化を見直す)
- 要件件数が増え1ファイル運用が扱いにくくなった時 (記録層と同条件)
- title/statement 変更の来歴連動が運用で重すぎた時
