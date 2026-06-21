---
name: commit
description: "Use when splitting changes into reviewable commits and writing Japanese conventional commit messages like feat(scope): Japanese description."
---

# commit

変更を管理しやすい単位へ分けてコミットする。

## 原則

- レビューしやすい単位で分ける
- 目的が違う変更を同じコミットへ混ぜない
- 生成物、文書、実装、試験を必要に応じて分ける
- コミット前に差分を確認
- ユーザー未承認の無関係差分は含めない

## メッセージ

形式:

```text
<type>(<scope>): <日本語説明>
```

例:

```text
feat(skill): 実装監督スキルを追加
fix(hook): 差分確認の対象漏れを修正
docs(requirements): 要件文書の保存規則を追記
```

## type

- `feat`: 機能追加
- `fix`: 不具合修正
- `docs`: 文書
- `refactor`: 振る舞いを変えない整理
- `test`: 試験
- `chore`: 雑務

## 手順

1. 状態確認
   - 作業ツリー
   - 未追跡ファイル
   - 差分

2. 変更分類
   - 目的別
   - 影響範囲別
   - レビュー単位別

3. コミット案提示
   - 含めるファイル
   - 含めないファイル
   - メッセージ
   - 理由

4. 作成
   - 必要な範囲だけ `git add`
   - コミット直前に差分確認
   - コミット後に状態確認

## 完了

- 作成したコミット
- 残した差分
- 次の推奨 skill 1-3 件
  - `handoff`: 作業継続情報を残す
  - `verify`: コミット後確認が必要
  - `review`: 履歴単位で再確認する
