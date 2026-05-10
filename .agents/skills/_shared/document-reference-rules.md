# 文書参照と個票ルール

## 1件 1個票

- 1 要件 = 1 requirements 個票
- 1 判断 = 1 ADR 個票
- 1 共通パターン = 1 patterns 個票
- 1 外部サービス = 1 integrations 個票
- 1 plan task = 1 plan task 個票
- 複数の独立した対象を 1 個票に混在させない

## 対応する index.md

- `docs/project/architecture.md` を更新したら `docs/project/index.md` を同じ変更で更新する
- `docs/project/tech-stack.md` を更新したら `docs/project/index.md` を同じ変更で更新する
- `docs/project/validation.md` を更新したら `docs/project/index.md` を同じ変更で更新する
- `docs/project/glossary/` 配下の個票を更新または作成したら `docs/project/glossary/index.md` を同じ変更で更新する
- `docs/project/requirements/` 配下の個票を更新または作成したら `docs/project/requirements/index.md` を同じ変更で更新する
- `docs/project/specs/` 配下の個票を更新または作成したら、配置先に対応する `index.md` を同じ変更で更新する
- `docs/project/patterns/` 配下の個票を更新または作成したら `docs/project/patterns/index.md` を同じ変更で更新する
- `docs/project/integrations/` 配下の個票を更新または作成したら `docs/project/integrations/index.md` を同じ変更で更新する
- `docs/project/teams/` 配下の個票を更新または作成したら `docs/project/teams/index.md` を同じ変更で更新する
- `docs/project/research/` 配下の個票を更新または作成したら `docs/project/research/index.md` を同じ変更で更新する
- `docs/project/proposals/active/` または `docs/project/proposals/archive/` 配下の個票を更新または作成したら `docs/project/proposals/index.md` を同じ変更で更新する
- `docs/project/adr/active/` または `docs/project/adr/archive/` 配下の個票を更新または作成したら `docs/project/adr/index.md` を同じ変更で更新する
- `docs/project/plan/` 配下の plan、phase、task を更新または作成したら、同じ階層の `index.md` と上位の `index.md` を同じ変更で更新する

## index の参照の書き方

- `## 参照` には `- `相対パス`: 説明` 形式で書く
- `active/` と `archive/` を持つカテゴリは、ディレクトリ参照と個票参照の両方を保つ
- 個票がまだない場合は、現在個票がないことと追加時の置き場所を明記する
- 個票の追加、更新、改名、移動、保管をしたら、同じターンで対応する `index.md` まで更新する
