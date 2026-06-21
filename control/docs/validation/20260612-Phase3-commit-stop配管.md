# Phase 3 commit / stop 配管検証

## 状態

完了。機械検証・実機 Codex での発火とも確証。

完了検証の中身 (verify 済みか・未記録判断) は Phase 4 の状態依存。本スライスは配管と誘導まで
(`docs/decisions/20260612-Phase3-hook実装.md`)。

## 実機確証 (2026-06-12)

target repo の対話 Codex セッションで全配管が成立:

- SessionStart: owox の文脈 (Goal + Read next) が hook context として注入された
- PreToolUse + git commit: commit 直前に完了確認の additionalContext が AI へ届いた。commit 自体は止まらず通った (誘導であってブロックでない)
- Stop: ターン終了時に Stop hook が継続をかけ、完了チェックリストが継続プロンプトとして AI へ渡った。AI は検証手段の確認 (git diff --check 等) を行い、2 度目の停止で受理。無限ループは起きず (stop_hook_active によるループ防止が実機で成立)

結論: commit リマインダ・Stop 継続・ループ防止が実機で成立。Phase 4 で機械ゲートへ格上げする土台が確証された。

## 検証日時

2026-06-12 (機械検証分)

## 検証した target repo

`/workspace/product/target`。

## 生成した target harness

`owox generate codex /workspace/product/target`。hooks.json に Stop を追加 (matcher 無し、command は owox 直接呼び)。SessionStart / PreToolUse は前スライスのまま。

owox: release を `/usr/local/bin/owox` へ再配置。

## 実行した確認 (機械)

`owox hook` に各入力 JSON を stdin で渡して出力を確認:

- PreToolUse + git commit: `git commit -m wip` で allow + additionalContext (`hookSpecificOutput.hookEventName=PreToolUse` + additionalContext に完了確認) を返し終了コード 0。止めない
- PreToolUse + 通常 Bash: `git status` は出力無し・終了コード 0 (素通り)。git commit-tree (plumbing) は commit 扱いしない
- Stop + 未継続: `stop_hook_active=false` で `{"decision":"block","reason":...}` を返し終了コード 0。reason が完了チェックリスト
- Stop + 既継続: `stop_hook_active=true` で出力無し・終了コード 0 (受理。ループしない)
- 単体テスト: commit でリマインダ / 不可逆は commit より優先で deny / Stop は未継続→継続・既継続→受理
- cargo test 23 本通過 / clippy 警告なし / fmt 済み

出力はいずれも一次情報の契約 (PreToolUse additionalContext、Stop の decision=block) と一致。

## 結果 (機械)

commit 認識 → 完了確認の誘導、Stop → 1 回継続の配管が成立。機械ブロックでなく誘導 (Phase 4 で機械ゲート化)。

## 確認手順 (人間、TTY のある実ターミナルで)

`codex exec` は未承認 hook をスキップするため対話起動で確認する。定義ハッシュが変わったため hook 再承認が要る。

1. `cd /workspace/product/target` → `codex` 対話起動 → hook 再承認
2. commit 確認: AI に何か commit させ、PreToolUse の完了確認 (additionalContext) が AI に届くか。commit 自体は止まらないこと
3. Stop 確認: ターンが終わろうとした時、完了チェックリストで 1 度だけ継続が促されるか。2 度目の停止で受理され無限ループしないこと
4. 誤発火確認: 通常作業 (status / ls 等) が妨げられないこと

## 残る観察点

- Stop の継続は今は毎ターン 1 回発火する (状態が無く verify 済みか判定できないため)。実機では適切に働いたが、正常ターンで体感が煩わしくないかは運用で見極める。煩わしければ Phase 4 で「未検証の時だけ継続」へ格上げして解消する

## 次に確認すること

- Phase 4 で commit/stop を機械ゲート (verify 状態・判断記録で block) へ格上げ
- 残る Phase 3: rules.md の正規表現追記 (target 固有の不可逆操作)、差分要約・作業メモ
