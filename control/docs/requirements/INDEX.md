# requirements

要件、実装計画、完了条件を置く。

## 置くもの

- 作るもの
- 作らないもの
- 実装順
- 完了条件
- 人間判断が必要な未決事項

## 読む時

- 作業開始時
- 実装範囲を確認する時
- 完了判定する時

## 文書

- `20260611-core-要件.md`: owox-harness 要件 (実装順で初期・完全版を段階化)
- `20260612-canon-正本構造.md`: 第1段階の正本 (brand 拡張 / rules / context / targets) の型確定とルート指示ファイル本実装
- `20260613-Phase5-スキルと入口.md`: Phase5 (スキル / 入口 / setup) の要件。Codex native Skills 整合と決定事項・未決事項
- `20260614-Phase6-検証を閉じる.md`: Phase6 (検証を閉じる) の要件。4スライス・要件の状態モデル提案・要件完了の機械判定・quality.toml・多視点レビュー枠組み
- `20260614-Phase7-腐らず成長する.md`: Phase7 (腐らず成長する) の要件。4スライス・腐敗検知の中核 (タスク5種 + 来歴鮮度・閾値は quality.toml の decay 節・phase 適応)・コード拡大・測定可視化とブランド検証・経験 IO
- `20260616-Phase8-経験を厚くする.md`: Phase8 (経験を厚くする) の要件。調査知識層 (調査日/ソース/要約必須・経過日数で鮮度・オンデマンド読み・supersede 専用・ドメイン分離)・スキルテスト是正 (契約 lint を最低ゲート・LLM 評価は Phase10)・パターンからスキル育成 (使用履歴+助言)・canon 編集の統一維持
- `20260621-Phase10-配布と条件付き機能.md`: Phase10 D群のうち配布と release.toml の要件。owox-harness 自身の配布 (main の `.owox-version` 変更起点の release workflow 4ビルド・setup.sh/install.ps1・SHA256SUMS・`.owox-version` 追従の版・owox --version)・release.toml 任意正本 (policy/version/artifacts/checks・型検証)・release.check MCP tool・人間向け導入文書。eval とチーム権限は対象外 (別スライス)。完了条件は機械確証 (cargo/clippy/fmt/shellcheck) と、main の `.owox-version` 変更 merge 配布実走+別環境導入を人間が確認した後の製品完成 (M6)。設計の真実源は `docs/decisions/20260621-Phase10-配布とrelease正本.md`
- `20260620-Phase9-自律度とオーケストレーション.md`: Phase9 別スライス。MCP コンテキスト削減 (生成後除去で title/default を削り $schema は残す・冗長除去・description 短縮)・自動承認パス再設計 (既定は architecture 軸導出 flat オン/layered オフ・同意源 profile 永続/session 限りの2系統・対象は層×不可逆の積・後追いキューは来歴一本)・オーケストレーション (正本 agents.toml で役割5×変種+ティア+sandbox+許可層・床の能動提示・Codex は custom agent_type spawn 不可と実機判明しフォールバック=組込み型+prompt 注入を採用し .codex/agents 生成は撤去)。作るもの/完了条件/未決を束ねる。真実源は同名 decisions・実機は `docs/validation/20260620-Phase9-オーケストレーション実機.md`
- `20260618-Phase9-性質軸適応とブランチ記憶.md`: Phase9 のうち性質軸適応機構とブランチ作業記憶層の要件 (設計確定・未実装。マルチ CLI/オーケストレーションは別スライス)。profile.toml と軸解決 (4軸2値・束→active モジュール・既定束フル)・kickoff 束導線+逆生成の厚い検出 (draft+根拠で人間確定)・4方法論の既存面実装 (req スキル・優先度属性・stage/layer タグ・種類タグ機能/非機能・quality.toml 層境界3分解・PRFAQ は要件正本へ蒸留し文書を別に残さない)・ゲート合成 (層×phase の max・guarded v1=4段)・ブランチ作業記憶層 (work/ 拡張ブランチ別キー・自動削除せず stale 剪定・床非注入・同 repo 同ディスク共有)
- `20260625-refs-参照IDと参照検査.md`: 草案。ソースコードや文書から要件・来歴を参照する記法 (`owox:req:` / `owox:dec:`) と、参照検査・逆引きを既存の `verify.run` / `context` へ寄せる方針。参照専用 tool を増やさない前提
- `20260625-diff-差分文脈.md`: コア実装済み。`context scope="diff"` でブランチの変更地図を返す方針。変更ファイル一覧・正本面抜粋・review_hints 実装済み。参照ID検査 (reference_summary) は未確認
- `20260625-glossary-用語オンデマンド.md`: 草案。用語定義を `SessionStart` 常時注入から外し、発話・読取前スキャン・`glossary.lookup` で必要時だけ届ける方針。`Read` や `cat` などの読取前に対象ファイルを軽く用語走査し、登録すべき用語候補は `verify.run` / `next` / `context scope="diff"` に根拠つきで出す。新規 glossary tool は増やさない前提
- `20260625-gardening-harness庭師.md`: 草案。育った `target harness` の用語・指針・スキル・MCP tool 説明・床文脈・記憶の重複/低利用/肥大/陳腐化/壊れを検出し、`verify.run` / `next` / `context scope="diff"` / `review.lenses` へ寄せる方針。自動削除や新規 pruning 専用 tool は作らない前提
- `20260625-skill-script化誘導.md`: 検出ロジック実装済み。RoutineKind::ScriptSkill / Confidence / is_script_skill_candidate / verify.run の routine_suggestions 出力・next 露出は実装済み。usage ログの安全分類語彙・warning 出力先は未確認
- `20260625-state-状態別rules.md`: 草案。新しい state 軸は増やさず、既存 `phase` に common rules と phase rules を持たせる方針。`SessionStart` / `rules.lookup` / `PreToolUse` は現在 phase の rules だけを届け、gate の厳しさは既存 phase enforcement に任せる
- `20260625-sessionstart-床最適化.md`: 草案。`SessionStart` を Vision・応答言語・phase・canon 直読み禁止・入口地図・短い current pressure に絞り、orchestration・用語/指針/skill 一覧・詳細ルーティングを skill や lookup へ逃がす方針
- `20260625-tools-tool整理.md`: 骨格実装済み・残論点決着。入口 skill 13個・context scope diff/codebase・routine script-skill 検出は実装済み。カテゴリ露出と mode 統合は `docs/decisions/20260628-tool整理仕上げと仕様管理取り込み.md` で決着 (露出せず・統合見送り)
- `20260625-memo-伝言メモ不要.md`: 草案。伝言メモ専用 tool や新ストアは作らず、判断は `decision.record`、作業メモは `task.note` / `branch.note`、調査結果は `knowledge.add`、skill 改善は `skill.remember`、引き継ぎは `$handoff` へ分類する方針
- `20260625-rules-practices読取タイミング.md`: 草案。ユーザー発話の語彙で intent を判定せず、`always` / `path` / `operation` の3 trigger で rules / practices を届ける方針。操作種類は tool 入力・path・差分から機械判定し、i18n は不要にする
- `20260625-codebase-コードベース索引.md`: 草案。コードベース調査結果を永続記憶でなく索引 cache として扱い、`context scope="codebase"` で本文なしの repo 地図を返す方針。腐ったら再計算し、耐久事実は `knowledge.add`、判断は `decision.record` へ昇格する
- `20260625-mission-kickoff.md`: 草案。session 任務は常に 1 つあり既定は `work`、`kickoff` は `mission.start type="kickoff"` で手動切替する方針。切替後の `next` / `context` / `verify.run` などは未確定事項の洗い出しと1論点ずつの判断詰めへ最適化される。全質問に AI の推奨案と理由を必ず添え、自動正本書込はしない
- `20260627-manifest-正本索引方針.md`: 草案。初期実装では専用 manifest を作らず、`context scope="codebase"` と codebase index cache を正本探索の入口にする方針
