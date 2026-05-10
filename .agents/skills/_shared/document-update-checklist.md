# 正本更新の横断確認

- requirement を変えたら spec / validation / ADR への影響を確認する
- spec を変えたら code / test / pattern / validation / ADR への影響を確認し、必要なら ADR を更新または作成する
- architecture を変えたら spec / code / test / validation / tech-stack を確認する
- tech-stack を変えたら architecture / validation / ADR への影響を確認し、必要なら ADR を更新または作成する
- integration を追加または変更したら `.agents/project.md` と `docs/project/integrations/` を確認する
- team を追加または変更したら `.agents/project.md` と `docs/project/teams/` を確認する
- 個票を追加、更新、改名、移動、保管したら同カテゴリの `index.md` を同じ変更で更新する
- proposal を追加または変更したら requirement / spec / ADR への昇格と ADR 更新要否を確認し、必要なら ADR を更新または作成する
- research で重要判断が固まったら proposal または ADR への昇格を確認する
- plan を追加または変更したら requirement / spec / ADR / validation / pattern / proposal / research への参照と昇格要否を確認する
