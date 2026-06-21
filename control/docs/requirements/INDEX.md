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
- `20260621-Phase10-配布と条件付き機能.md`: Phase10 D群のうち配布と release.toml の要件。owox-harness 自身の配布 (release workflow 4ビルド・setup.sh/install.ps1・SHA256SUMS・版 0.1.0・owox --version)・release.toml 任意正本 (policy/version/artifacts/checks・型検証)・release.check MCP tool・人間向け導入文書。eval とチーム権限は対象外 (別スライス)。完了条件は機械確証 (cargo/clippy/fmt/shellcheck) と、実 tag push 配布実走+別環境導入を人間が確認した後の製品完成 (M6)。設計の真実源は `docs/decisions/20260621-Phase10-配布とrelease正本.md`
- `20260620-Phase9-自律度とオーケストレーション.md`: Phase9 別スライス。MCP コンテキスト削減 (生成後除去で title/default を削り $schema は残す・冗長除去・description 短縮)・自動承認パス再設計 (既定は architecture 軸導出 flat オン/layered オフ・同意源 profile 永続/session 限りの2系統・対象は層×不可逆の積・後追いキューは来歴一本)・オーケストレーション (正本 agents.toml で役割5×変種+ティア+sandbox+許可層・床の能動提示・Codex は custom agent_type spawn 不可と実機判明しフォールバック=組込み型+prompt 注入を採用し .codex/agents 生成は撤去)。作るもの/完了条件/未決を束ねる。真実源は同名 decisions・実機は `docs/validation/20260620-Phase9-オーケストレーション実機.md`
- `20260618-Phase9-性質軸適応とブランチ記憶.md`: Phase9 のうち性質軸適応機構とブランチ作業記憶層の要件 (設計確定・未実装。マルチ CLI/オーケストレーションは別スライス)。profile.toml と軸解決 (4軸2値・束→active モジュール・既定束フル)・kickoff 束導線+逆生成の厚い検出 (draft+根拠で人間確定)・4方法論の既存面実装 (req スキル・優先度属性・stage/layer タグ・種類タグ機能/非機能・quality.toml 層境界3分解・PRFAQ は要件正本へ蒸留し文書を別に残さない)・ゲート合成 (層×phase の max・guarded v1=4段)・ブランチ作業記憶層 (work/ 拡張ブランチ別キー・自動削除せず stale 剪定・床非注入・同 repo 同ディスク共有)
