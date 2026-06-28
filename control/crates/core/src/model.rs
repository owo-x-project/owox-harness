//! 正本モデル。`.owox/` の型付き表現。
//!
//! 第1段階: brand / rules / context / targets。
//! prose の正本 (brand/rules/context) は Markdown、machine 設定の targets は TOML で書き、
//! 読込後にこの型へ検証する (`docs/decisions/20260612-正本フォーマット.md`)。

use std::collections::BTreeMap;

use serde::Deserialize;

use crate::agents::Agents;
use crate::markdown::{Doc, split_pair};
use crate::profile::Profile;
use crate::quality::Quality;
use crate::release::Release;

/// 正本全体。`.owox/` 配下の各ファイルを型で束ねる。
///
/// brand は必須。それ以外は任意 (無くても生成が通る。段階的に正本を増やせる)。
/// Default は test・生成器の引数用 (load 経路は実ファイル由来で未使用)。
#[derive(Debug, Clone, Default)]
pub struct Canon {
    pub brand: Brand,
    pub rules: Rules,
    pub context: Context,
    pub glossary: Glossary,
    /// 経験から育つ運用指針 (成長層)。固定 rules と別管理。
    pub practices: Practices,
    pub targets: Targets,
    pub verify: VerifyConfig,
    pub quality: Quality,
    pub state: State,
    pub settings: Settings,
    /// プロジェクトの性質 (固定)。phase の時間軸と直交する性質軸
    /// (`docs/decisions/20260618-Phase9-性質軸適応機構.md`)。
    pub profile: Profile,
    /// オーケストレーション正本。役割 × 変種の2次元定義
    /// (`docs/decisions/20260620-オーケストレーション具体化.md`)。
    pub agents: Agents,
    /// 配布運用がある対象プロジェクトの配布方針 / 版 / 成果物検証 (任意)
    /// (`docs/decisions/20260621-Phase10-配布とrelease正本.md`)。
    pub release: Release,
}

/// config.toml のトップレベル設定。targets / verify と並ぶ machine 設定。
///
/// language: 人間へ応答する言語。生成物の指示文は英語固定だが、応答だけ追従させる
/// (指示言語と応答言語を分離。`docs/decisions/20260613-Phase5-実機検証の是正.md`)。
/// 未設定ならモデル既定 (注入しない)。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Settings {
    pub language: Option<String>,
    /// 床コンテキストの注入上限 (`[context]`)。肥大化時に自動降格する閾値。
    pub context: ContextLimits,
}

impl Settings {
    /// config.toml のトップレベル `language` と `[context]` を読む。
    pub fn from_toml(text: &str) -> Result<Settings, String> {
        let raw: ConfigRaw = toml::from_str(text).map_err(|e| e.to_string())?;
        let d = ContextLimits::default();
        Ok(Settings {
            language: raw.language.filter(|s| !s.trim().is_empty()),
            context: ContextLimits {
                practices_floor_max: raw
                    .context
                    .practices_floor_max
                    .unwrap_or(d.practices_floor_max),
                glossary_floor_max: raw
                    .context
                    .glossary_floor_max
                    .unwrap_or(d.glossary_floor_max),
            },
        })
    }
}

/// 床コンテキストの注入上限。件数がこれを超えたら床を縮め、残りはオンデマンドへ降格する
/// (`docs/decisions/20260621-Phase9-経験層スケールとGitHub連携とkickoff束ね.md`)。
/// 既定は十分大きく、小規模では現状どおり全注入し発見性を最優先する。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextLimits {
    /// practices を床へ全注入する上限。超過なら新しい順に上位だけ床へ・残りは practice.lookup。
    pub practices_floor_max: usize,
    /// glossary 用語名を床へ全列挙する上限。超過なら一覧を出さず glossary.lookup へ寄せる。
    pub glossary_floor_max: usize,
}

impl Default for ContextLimits {
    fn default() -> Self {
        // 十分大きく取り、肥大化した時だけ効く。実機で調整する。
        ContextLimits {
            practices_floor_max: 40,
            glossary_floor_max: 60,
        }
    }
}

/// プロジェクトの状態。機械強制の厳しさを段階適応させる
/// (`docs/decisions/20260611-制御方針.md`)。人間宣言が正で `.owox/state.toml` に置く。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct State {
    pub phase: Phase,
}

impl State {
    /// state.toml を読む。`phase = "initial"`。未設定・空なら initial。
    pub fn from_toml(text: &str) -> Result<State, String> {
        let raw: StateRaw = toml::from_str(text).map_err(|e| e.to_string())?;
        let phase = match raw.phase {
            Some(p) => Phase::parse(&p)?,
            None => Phase::default(),
        };
        Ok(State { phase })
    }

    /// state.toml の本文へ描画する。
    pub fn to_toml(&self) -> String {
        format!("phase = \"{}\"\n", self.phase.as_str())
    }
}

#[derive(Deserialize, Default)]
struct StateRaw {
    phase: Option<String>,
}

/// 状態の段階。早い順 (緩い → 厳しい)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Phase {
    /// 初期。試行錯誤。機械強制は緩い (警告寄り)。既定。
    #[default]
    Initial,
    /// 安定。
    Stable,
    /// 保守。回帰防止優先。機械強制は厳しい (block 寄り)。
    Maintenance,
}

impl Phase {
    /// 文字列から読む。
    pub fn parse(value: &str) -> Result<Phase, String> {
        match value.trim() {
            "initial" => Ok(Phase::Initial),
            "stable" => Ok(Phase::Stable),
            "maintenance" => Ok(Phase::Maintenance),
            other => Err(format!(
                "phase は initial / stable / maintenance のみ: {other}"
            )),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Phase::Initial => "initial",
            Phase::Stable => "stable",
            Phase::Maintenance => "maintenance",
        }
    }
}

/// brand.md: 目的 (Vision) ・価値・方針・対象外・成功条件・表記文体。
///
/// 用語集は別ファイル glossary.md に分けた (用語を直読みする時 brand の他項目を巻き込まない)。
#[derive(Debug, Clone, Default)]
pub struct Brand {
    /// 目的。プロジェクトを導く永続の目的。セッション目標ではない。必須。
    pub vision: String,
    /// 価値。
    pub values: Vec<String>,
    /// 方針。
    pub principles: Vec<String>,
    /// 対象外。広がりすぎを防ぐ。
    pub non_goals: Vec<String>,
    /// 成功条件。
    pub success_criteria: Vec<String>,
    /// 表記・文体の方針。
    pub style: Vec<String>,
}

impl Brand {
    /// brand.md を読み型へ検証する。未知見出し・Vision 欠落はエラー。
    pub fn from_markdown(text: &str) -> Result<Brand, String> {
        let mut doc = Doc::parse(text);

        let vision = doc
            .take("Vision")
            .map(|s| s.text())
            .filter(|t| !t.is_empty())
            .ok_or("Vision セクションが必須")?;

        let brand = Brand {
            vision,
            values: doc.take("Values").map(|s| s.list()).unwrap_or_default(),
            principles: doc.take("Principles").map(|s| s.list()).unwrap_or_default(),
            non_goals: doc.take("Non-goals").map(|s| s.list()).unwrap_or_default(),
            success_criteria: doc
                .take("Success criteria")
                .map(|s| s.list())
                .unwrap_or_default(),
            style: doc.take("Style").map(|s| s.list()).unwrap_or_default(),
        };

        reject_unknown(&doc)?;
        Ok(brand)
    }
}

/// 用語集 1 件。`用語: 説明`、別名つきは `用語 | 別名1 | 別名2: 説明` で書く。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlossaryEntry {
    /// 用語 (正規名)。床へ出すのはこれだけ。
    pub term: String,
    /// 別名。正規名と同義として照合する (出現時 push・lookup)。床へは出さない。
    pub aliases: Vec<String>,
    /// 説明。
    pub definition: String,
}

impl GlossaryEntry {
    /// `用語 | 別名 | 別名: 説明` を読む。コロン左をパイプで分け、先頭が用語・残りが別名。
    /// パイプはコロン左にだけ効くので説明にパイプがあっても影響しない。
    fn parse(item: &str) -> GlossaryEntry {
        let (name, definition) = split_pair(item);
        let mut parts = name.split('|').map(|s| s.trim().to_string());
        let term = parts.next().unwrap_or_default();
        let aliases = parts.filter(|s| !s.is_empty()).collect();
        GlossaryEntry {
            term,
            aliases,
            definition,
        }
    }

    /// 用語または別名のいずれかが `lower_text` (小文字化済み) に部分一致するか。
    /// 照合は大文字小文字を無視する。
    pub fn matches(&self, lower_text: &str) -> bool {
        if self.term.is_empty() {
            return false;
        }
        lower_text.contains(&self.term.to_lowercase())
            || self
                .aliases
                .iter()
                .any(|a| !a.is_empty() && lower_text.contains(&a.to_lowercase()))
    }
}

/// 禁止語 1 件。`パターン: 理由` で書く。
///
/// パターンは正規表現 (owox は irreversible・boundaries と揃え正規表現で持つ)。
/// 読込時に妥当性を検証する。run_brand が追跡テキストへ照合しブランド違反を報告する
/// (`docs/decisions/20260614-Phase7-測定可視化とブランド検証.md`)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForbiddenTerm {
    /// 禁止する正規表現。読込時に妥当性を検証済み。
    pub pattern: String,
    /// なぜ禁止か・代替。人間向け。
    pub reason: String,
}

/// glossary.md: プロジェクト固有の用語集。brand から分けた独立正本。
///
/// 用語名は床コンテキストに常時、定義は必要時に push hook が届ける (段階的開示)。
/// 用語を直読みする時、brand の他項目を巻き込まないよう別ファイルにする。
/// 予約見出し `Forbidden` 配下は禁止語 (用語集を用語+定義+禁止語の正本にする)。
#[derive(Debug, Clone, Default)]
pub struct Glossary {
    pub entries: Vec<GlossaryEntry>,
    /// 禁止語。予約見出し `Forbidden` 配下の `- パターン: 理由`。
    pub forbidden: Vec<ForbiddenTerm>,
}

impl Glossary {
    /// glossary.md を読む。予約見出し `Forbidden` 配下を禁止語、他の `## 見出し` 配下の
    /// `- 用語: 説明` を用語として全節から集める。見出しは任意 (用語を分類してよい)。
    /// 禁止語の正規表現は読込時に妥当性を検証する (誤記を早期に弾く)。
    pub fn from_markdown(text: &str) -> Result<Glossary, String> {
        let mut doc = Doc::parse(text);
        let forbidden = match doc.take("Forbidden") {
            Some(section) => parse_forbidden(&section)?,
            None => Vec::new(),
        };
        let entries = doc
            .into_sections()
            .iter()
            .flat_map(|s| s.list())
            .map(|i| GlossaryEntry::parse(&i))
            .filter(|e| !e.term.is_empty())
            .collect();
        Ok(Glossary { entries, forbidden })
    }
}

/// 予約見出し `Forbidden` の節を読む。`- パターン: 理由` がエントリ。
/// パターンは正規表現として妥当か検証する。
fn parse_forbidden(section: &crate::markdown::Section) -> Result<Vec<ForbiddenTerm>, String> {
    let mut out = Vec::new();
    for item in section.list() {
        let (pattern, reason) = split_pair(&item);
        if pattern.is_empty() {
            continue;
        }
        regex::Regex::new(&pattern)
            .map_err(|e| format!("禁止語の正規表現が不正: {pattern}: {e}"))?;
        out.push(ForbiddenTerm { pattern, reason });
    }
    Ok(out)
}

/// 成長層の指針 1 件。`日付: 指針` で書く。
///
/// 経験から育つ運用指針。固定 rules.md と別管理で、AI が practice.add で育てる。
/// 日付で鮮度を測る (古さは見直し合図・捨てる根拠にしない。
/// `docs/decisions/20260614-Phase7-経験IOと二層ルール.md`)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Practice {
    /// いつ得た指針か (YYYYMMDD)。鮮度判定に使う。
    pub date: String,
    /// 指針本文。
    pub text: String,
}

/// practices.md: 経験から育つ運用指針 (成長層)。固定 rules.md と別管理。
///
/// 形式は memory.md と同じ `- 日付: 指針`。AI が practice.add で追記し、
/// 届け方は床コンテキストのライブ注入 (静的列挙しない)。
#[derive(Debug, Clone, Default)]
pub struct Practices {
    pub entries: Vec<Practice>,
}

impl Practices {
    /// practices.md を読む。`## 見出し` 配下の `- 日付: 指針` を全節から集める。
    /// 見出しは任意 (指針を分類してよい)。日付が無い項目は飛ばす。
    pub fn from_markdown(text: &str) -> Result<Practices, String> {
        let entries = Doc::parse(text)
            .into_sections()
            .iter()
            .flat_map(|s| s.list())
            .filter_map(|item| {
                let (date, body) = split_pair(&item);
                if date.is_empty() || body.is_empty() {
                    None
                } else {
                    Some(Practice { date, text: body })
                }
            })
            .collect();
        Ok(Practices { entries })
    }
}

/// rules.md: 作業ルール。AI が従う変更方針と止まる境界。
///
/// 自由文の方針は箇条書き、機械が後で強制する境界は構造化エントリで持つ。
/// 全項目任意 (部分的に書ける)。
#[derive(Debug, Clone, Default)]
pub struct Rules {
    /// Common rules. Always delivered regardless of phase.
    pub common: Vec<String>,
    /// 初期 phase だけで効く rules。
    pub initial: Vec<String>,
    /// 安定 phase だけで効く rules。
    pub stable: Vec<String>,
    /// 保守 phase だけで効く rules。
    pub maintenance: Vec<String>,
    /// 変更方針。AI が変更を進める際の方針。
    pub change_policy: Vec<String>,
    /// 依存追加の条件。
    pub dependency_policy: Vec<String>,
    /// 削除基準。何を消してよいか、消す前にすること。
    pub deletion_policy: Vec<String>,
    /// 安全性。秘密情報・外部送信・危険操作の扱い。
    pub safety: Vec<String>,
    /// 不可逆操作。実行前に人間確認必須。`detect:` で機械検出パターンを持てる。
    pub irreversible: Vec<Irreversible>,
    /// 人間ゲート。AI が必ず人間へ戻す判断点。
    pub human_gate: Vec<HumanGate>,
}

impl Rules {
    /// rules.md を読み型へ検証する。未知見出しはエラー。
    pub fn from_markdown(text: &str) -> Result<Rules, String> {
        let mut doc = Doc::parse(text);

        let common = doc.take("Common").map(|s| s.list()).unwrap_or_default();
        let rules = Rules {
            common,
            initial: doc.take("Initial").map(|s| s.list()).unwrap_or_default(),
            stable: doc.take("Stable").map(|s| s.list()).unwrap_or_default(),
            maintenance: doc
                .take("Maintenance")
                .map(|s| s.list())
                .unwrap_or_default(),
            change_policy: doc
                .take("Change policy")
                .map(|s| s.list())
                .unwrap_or_default(),
            dependency_policy: doc
                .take("Dependency policy")
                .map(|s| s.list())
                .unwrap_or_default(),
            deletion_policy: doc
                .take("Deletion policy")
                .map(|s| s.list())
                .unwrap_or_default(),
            safety: doc.take("Safety").map(|s| s.list()).unwrap_or_default(),
            irreversible: match doc.take("Irreversible operations") {
                Some(section) => parse_irreversible(&section)?,
                None => Vec::new(),
            },
            human_gate: doc
                .take("Human gates")
                .map(|s| s.list().iter().map(|i| HumanGate::parse(i)).collect())
                .unwrap_or_default(),
        };

        reject_unknown(&doc)?;
        Ok(rules)
    }

    pub fn phase_rules(&self, phase: Phase) -> &[String] {
        match phase {
            Phase::Initial => &self.initial,
            Phase::Stable => &self.stable,
            Phase::Maintenance => &self.maintenance,
        }
    }
}

/// 不可逆操作の節を読む。
///
/// `- 操作: 理由` がエントリ、その下の `detect: <正規表現>` 行が任意の検出パターン。
/// 検出パターンは読込時に正規表現として妥当か検証する (誤記を早期に弾く)。
fn parse_irreversible(section: &crate::markdown::Section) -> Result<Vec<Irreversible>, String> {
    let mut entries: Vec<Irreversible> = Vec::new();
    for line in section.lines() {
        if let Some(item) = line.strip_prefix("- ") {
            let (operation, reason) = split_pair(item.trim());
            entries.push(Irreversible {
                operation,
                reason,
                detect: None,
            });
            continue;
        }

        // 箇条書きでない行は直前エントリの属性。
        let (key, value) = split_pair(line);
        match key.as_str() {
            "detect" => {
                let entry = entries
                    .last_mut()
                    .ok_or("detect: は不可逆操作の箇条書きの後に書く")?;
                regex::Regex::new(&value)
                    .map_err(|e| format!("detect の正規表現が不正: {value}: {e}"))?;
                entry.detect = Some(value);
            }
            other => return Err(format!("不可逆操作の未知のキー: {other}")),
        }
    }
    Ok(entries)
}

/// 不可逆操作 1 件。`操作: 理由` で書く。
///
/// 任意の `detect:` 行で検出用のコマンド正規表現を持てる。owox 同梱の既定検出器
/// に加え、target 固有の不可逆操作を機械検出させる
/// (`docs/decisions/20260612-Phase3-hook実装.md`)。detect が無いエントリは
/// 人間向けの記述のみ (注入文に出るが機械検出はしない)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Irreversible {
    /// 操作の説明 (人間が読んで分かる形)。
    pub operation: String,
    /// なぜ不可逆・確認必須か。
    pub reason: String,
    /// 検出用のコマンド正規表現 (任意)。読込時に妥当性を検証済み。
    pub detect: Option<String>,
}

/// 人間ゲート 1 件。`状況: 理由` で書く。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HumanGate {
    /// どんな時に人間へ戻すか。
    pub situation: String,
    /// なぜ人間判断が要るか。
    pub reason: String,
}

impl HumanGate {
    fn parse(item: &str) -> HumanGate {
        let (situation, reason) = split_pair(item);
        HumanGate { situation, reason }
    }
}

/// context.md: 文脈地図。作業・場所ごとに必読と適用ルールを示す。
///
/// 各エントリの届け方 (作業/場所に応じた注入) は Phase 3 (hook) ・Phase 4 (MCP) で実装。
/// 本 Phase は型確定のみ (`docs/decisions/20260612-段階的開示.md`)。
#[derive(Debug, Clone, Default)]
pub struct Context {
    pub entries: Vec<ContextEntry>,
}

impl Context {
    /// context.md を読み型へ検証する。各 `## 見出し` が 1 エントリ。
    pub fn from_markdown(text: &str) -> Result<Context, String> {
        let mut entries = Vec::new();
        for section in Doc::parse(text).into_sections() {
            entries.push(ContextEntry::from_section(&section)?);
        }
        Ok(Context { entries })
    }
}

/// 文脈地図 1 件。`## 見出し` がスコープ、配下の `key: value` が中身。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextEntry {
    /// スコープ。作業の説明、またはパス glob。
    pub scope: String,
    /// スコープの種類 (どう照合するか)。
    pub kind: ScopeKind,
    /// このスコープで読むべきファイル (`read:`)。
    pub reads: Vec<String>,
    /// このスコープで適用するルール・スタイル (`note:`)。
    pub notes: Vec<String>,
}

impl ContextEntry {
    fn from_section(section: &crate::markdown::Section) -> Result<ContextEntry, String> {
        let scope = section.heading().to_string();
        let mut kind = ScopeKind::Task;
        let mut reads = Vec::new();
        let mut notes = Vec::new();

        for item in section.list() {
            let (key, value) = split_pair(&item);
            match key.as_str() {
                "kind" => kind = ScopeKind::parse(&value)?,
                "read" => reads.push(value),
                "note" => notes.push(value),
                other => return Err(format!("{scope}: 未知のキー: {other}")),
            }
        }

        Ok(ContextEntry {
            scope,
            kind,
            reads,
            notes,
        })
    }
}

/// スコープの照合種別。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScopeKind {
    /// 作業の種類で照合する (既定)。
    Task,
    /// ファイルパス glob で照合する。
    Path,
}

impl ScopeKind {
    fn parse(value: &str) -> Result<ScopeKind, String> {
        match value {
            "task" => Ok(ScopeKind::Task),
            "path" => Ok(ScopeKind::Path),
            other => Err(format!("kind は task / path のみ: {other}")),
        }
    }
}

/// config.toml: machine 設定の正本。生成対象 CLI など設定をここへ集約する。
///
/// machine 設定で入れ子があるため、prose の正本と違い TOML で持つ
/// (`docs/decisions/20260612-正本フォーマット.md`)。
/// 今は targets (生成対象 CLI) のみ。今後の設定的な項目もこのファイルへ足す。
/// ティア → モデルの割り当ては Phase 8 のオーケストレーションで使う。
#[derive(Debug, Clone, Default)]
pub struct Targets {
    pub entries: Vec<TargetSpec>,
}

impl Targets {
    /// config.toml を読み targets を型へ検証する。`[targets.<cli>]` 配下が 1 CLI。
    /// targets 以外のトップレベル設定は無視する (各設定はそれぞれの読み手が解釈する)。
    pub fn from_toml(text: &str) -> Result<Targets, String> {
        let raw: ConfigRaw = toml::from_str(text).map_err(|e| e.to_string())?;
        let mut entries = Vec::new();
        for (name, t) in raw.targets {
            let mut models = Vec::new();
            for (tier, val) in t.models {
                let (model, reasoning_effort) = match val {
                    ModelValueRaw::ModelOnly(m) => (m, None),
                    ModelValueRaw::Full(f) => (f.model, f.reasoning_effort),
                };
                models.push(ModelTier {
                    tier,
                    model,
                    reasoning_effort,
                });
            }
            entries.push(TargetSpec {
                name,
                out_dir: t.out,
                models,
            });
        }
        Ok(Targets { entries })
    }
}

/// config.toml 全体の読込用。設定追加時はここへフィールドを足す。
/// 未知のトップレベルキーは無視 (将来の設定や他読み手の領域を壊さない)。
#[derive(Deserialize, Default)]
struct ConfigRaw {
    #[serde(default)]
    targets: BTreeMap<String, TargetRaw>,
    #[serde(default)]
    verify: VerifyRaw,
    /// 人間へ応答する言語 (任意)。未設定なら注入しない。
    #[serde(default)]
    language: Option<String>,
    /// 床コンテキストの注入上限 (任意)。`[context]`。
    #[serde(default)]
    context: ContextRaw,
}

/// `[context]` の生表現。各値は省略可で、省略時は ContextLimits の既定値。
#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct ContextRaw {
    practices_floor_max: Option<usize>,
    glossary_floor_max: Option<usize>,
}

/// TOML 読込用の素の表。`[targets.<cli>]` 配下。未知キーは弾く。
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TargetRaw {
    /// 出力先 (target repo ルートからの相対)。既定 ".".
    #[serde(default = "dot")]
    out: String,
    /// ティア → モデル (または model+reasoning_effort の組)。`[targets.<cli>.models]`。
    /// `fast = "mini"` (文字列) と `fast = { model = "mini", reasoning_effort = "low" }` の両形式を受ける。
    #[serde(default)]
    models: BTreeMap<String, ModelValueRaw>,
}

/// models の値。文字列形式 (model のみ) とテーブル形式 (model + reasoning_effort) の両対応。
#[derive(Deserialize)]
#[serde(untagged)]
enum ModelValueRaw {
    /// `fast = "mini"` 形式。
    ModelOnly(String),
    /// `fast = { model = "mini", reasoning_effort = "low" }` 形式。
    Full(ModelValueTableRaw),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ModelValueTableRaw {
    model: String,
    reasoning_effort: Option<String>,
}

fn dot() -> String {
    ".".to_string()
}

/// verify 設定の読込用。`[[verify.checks]]` を並べる。
#[derive(Deserialize, Default)]
struct VerifyRaw {
    #[serde(default)]
    checks: Vec<VerifyCheckRaw>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct VerifyCheckRaw {
    name: String,
    command: String,
}

/// verify の設定。検証完了の機械判定で実行する検査コマンド。
///
/// 検査コマンドは人間が config.toml の `[[verify.checks]]` へ明示列挙する
/// (tool 非依存・machine 設定は config.toml 集約。`docs/decisions/20260613-Phase4-tool記録層.md`)。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VerifyConfig {
    pub checks: Vec<VerifyCheck>,
}

impl VerifyConfig {
    /// config.toml の `[[verify.checks]]` を読む。
    pub fn from_toml(text: &str) -> Result<VerifyConfig, String> {
        let raw: ConfigRaw = toml::from_str(text).map_err(|e| e.to_string())?;
        let checks = raw
            .verify
            .checks
            .into_iter()
            .map(|c| VerifyCheck {
                name: c.name,
                command: c.command,
            })
            .collect();
        Ok(VerifyConfig { checks })
    }
}

/// 検査 1 件。name は人間向けラベル、command はシェルで実行する検査コマンド。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifyCheck {
    pub name: String,
    pub command: String,
}

/// 生成対象 CLI 1 件。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetSpec {
    /// CLI 識別子。
    pub name: String,
    /// 出力先 (target repo ルートからの相対)。既定 ".".
    pub out_dir: String,
    /// ティア → 具体モデルの割り当て。
    pub models: Vec<ModelTier>,
}

/// ティア → モデル 1 件。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelTier {
    /// ティア名 (fast / balanced / strong / reasoning 等)。
    pub tier: String,
    /// 具体モデル。
    pub model: String,
    /// Codex の model_reasoning_effort 値 (low / medium / high)。None なら親から継承。
    pub reasoning_effort: Option<String>,
}

/// 取り出されずに残った見出しがあればエラー (誤記・未知項目の検出)。
fn reject_unknown(doc: &Doc) -> Result<(), String> {
    let remaining = doc.remaining_headings();
    if remaining.is_empty() {
        Ok(())
    } else {
        Err(format!("未知の見出し: {}", remaining.join(", ")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn irreversible_detect_attaches_to_previous_entry() {
        let rules = Rules::from_markdown(
            "## Irreversible operations\n\
             - terraform destroy: tears down infra\n\
             detect: \\bterraform\\s+destroy\\b\n",
        )
        .expect("読める");
        assert_eq!(rules.irreversible.len(), 1);
        let entry = &rules.irreversible[0];
        assert_eq!(entry.operation, "terraform destroy");
        assert_eq!(entry.detect.as_deref(), Some(r"\bterraform\s+destroy\b"));
    }

    #[test]
    fn entry_without_detect_has_none() {
        let rules = Rules::from_markdown("## Irreversible operations\n- some op: a reason\n")
            .expect("読める");
        assert_eq!(rules.irreversible[0].detect, None);
    }

    #[test]
    fn invalid_detect_regex_is_rejected_at_load() {
        // 誤記の正規表現は読込時に弾く (早期検出)。
        let err =
            Rules::from_markdown("## Irreversible operations\n- op: reason\ndetect: (unclosed\n")
                .unwrap_err();
        assert!(err.contains("detect"), "{err}");
    }

    #[test]
    fn detect_before_any_entry_is_rejected() {
        let err =
            Rules::from_markdown("## Irreversible operations\ndetect: \\bx\\b\n").unwrap_err();
        assert!(err.contains("detect"), "{err}");
    }

    #[test]
    fn settings_reads_language() {
        let s = Settings::from_toml("language = \"Japanese\"\n").unwrap();
        assert_eq!(s.language.as_deref(), Some("Japanese"));
    }

    #[test]
    fn glossary_routes_forbidden_and_terms() {
        // 予約見出し Forbidden 配下は禁止語、他の見出しは用語として読む。
        let g = Glossary::from_markdown(
            "## Glossary\n\
             - canon: source of truth\n\n\
             ## Forbidden\n\
             - \\bfoobar\\b: 造語禁止\n",
        )
        .expect("読める");
        assert_eq!(g.entries.len(), 1);
        assert_eq!(g.entries[0].term, "canon");
        assert_eq!(g.forbidden.len(), 1);
        assert_eq!(g.forbidden[0].pattern, r"\bfoobar\b");
        assert_eq!(g.forbidden[0].reason, "造語禁止");
    }

    #[test]
    fn glossary_invalid_forbidden_regex_rejected_at_load() {
        let err = Glossary::from_markdown("## Forbidden\n- (unclosed: reason\n").unwrap_err();
        assert!(err.contains("禁止語"), "{err}");
    }

    #[test]
    fn glossary_parses_pipe_aliases() {
        // コロン左をパイプで分け、先頭が用語・残りが別名。説明のコロンは影響しない。
        let g = Glossary::from_markdown(
            "## Glossary\n- target harness | th | harness output: generated files: see docs\n",
        )
        .expect("読める");
        assert_eq!(g.entries.len(), 1);
        assert_eq!(g.entries[0].term, "target harness");
        assert_eq!(g.entries[0].aliases, vec!["th", "harness output"]);
        assert_eq!(g.entries[0].definition, "generated files: see docs");
        assert!(g.entries[0].matches(&"use the th here".to_lowercase()));
        assert!(!g.entries[0].matches(&"unrelated text".to_lowercase()));
    }

    #[test]
    fn glossary_without_forbidden_is_empty() {
        let g = Glossary::from_markdown("## Glossary\n- canon: x\n").expect("読める");
        assert!(g.forbidden.is_empty());
    }

    #[test]
    fn practices_collect_dated_entries() {
        let p = Practices::from_markdown(
            "# Practices\n\n## Practices\n- 20260614: add a regression test\n- 20260610: prefer small diffs\n",
        )
        .expect("読める");
        assert_eq!(p.entries.len(), 2);
        assert_eq!(p.entries[0].date, "20260614");
        assert_eq!(p.entries[0].text, "add a regression test");
    }

    #[test]
    fn practices_skip_undated_items() {
        // 日付が無い項目は飛ばす (鮮度を測れない)。
        let p = Practices::from_markdown("## Practices\n- no date here\n").expect("読める");
        assert!(p.entries.is_empty());
    }

    #[test]
    fn settings_language_absent_or_blank_is_none() {
        // 未設定・空白は None (注入しない)。他キーがあっても language だけ見る。
        assert_eq!(Settings::from_toml("").unwrap().language, None);
        assert_eq!(
            Settings::from_toml("language = \"  \"\n").unwrap().language,
            None
        );
        assert_eq!(
            Settings::from_toml("[targets.codex]\nout = \".\"\n")
                .unwrap()
                .language,
            None
        );
    }
}
