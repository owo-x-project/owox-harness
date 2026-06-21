//! profile.toml: プロジェクトの性質 (固定)。phase の時間軸と直交する性質軸
//! (`docs/decisions/20260618-Phase9-性質軸適応機構.md`)。
//!
//! 性質は立ち上げで決まりほぼ固定。state.toml (可変) と別ファイルにして関心を分ける。
//! 4軸 (requirements-shape / prioritization / delivery / architecture) を基本単位にし、
//! 各軸が1モジュールを on/off/差し替える。プリセット (束) は軸選択の名前付き束にすぎない。
//!
//! 軸の値は将来の追加余地のため列挙集合として型設計する (v1 は各軸2値・追加はコード変更)。
//! 軸→active モジュールの解決は floor_context 合成と同型の純関数。

use std::collections::BTreeMap;
use std::path::Path;

use serde::Deserialize;
use serde_json::json;

use crate::envelope::Envelope;
use crate::record::{DecisionLinks, DecisionStatus, RecordInput, record_decision};

/// 要件の形。PRFAQ で起草するか軽量に書くか。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequirementsShape {
    /// プレスリリースと FAQ を先に書き承認してから着手する。
    Prfaq,
    /// 軽量に書く。
    Lightweight,
}

/// 優先順位の付け方。理想から逆算するか積み上げるか。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Prioritization {
    /// 理想先行。最初に理想の優先順位を決め逆算する。
    IdealFirst,
    /// 積み上げ。
    Incremental,
}

/// 届け方。段階に割るか連続で出すか。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Delivery {
    /// 段階化。大きな塊を段に割って届ける。
    Phased,
    /// 連続。
    Continuous,
}

/// 構造。層を分けるか平らにするか。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Architecture {
    /// 層分け (クリーンアーキ)。quality.toml 層境界を唯一の層真実に使う。
    Layered,
    /// 平ら。層機構は全 off。
    Flat,
}

impl RequirementsShape {
    pub fn parse(value: &str) -> Result<Self, String> {
        match value.trim() {
            "prfaq" => Ok(Self::Prfaq),
            "lightweight" => Ok(Self::Lightweight),
            other => Err(format!(
                "requirements-shape は prfaq / lightweight のみ: {other}"
            )),
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Prfaq => "prfaq",
            Self::Lightweight => "lightweight",
        }
    }
}

impl Prioritization {
    pub fn parse(value: &str) -> Result<Self, String> {
        match value.trim() {
            "ideal-first" => Ok(Self::IdealFirst),
            "incremental" => Ok(Self::Incremental),
            other => Err(format!(
                "prioritization は ideal-first / incremental のみ: {other}"
            )),
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Self::IdealFirst => "ideal-first",
            Self::Incremental => "incremental",
        }
    }
}

impl Delivery {
    pub fn parse(value: &str) -> Result<Self, String> {
        match value.trim() {
            "phased" => Ok(Self::Phased),
            "continuous" => Ok(Self::Continuous),
            other => Err(format!("delivery は phased / continuous のみ: {other}")),
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Phased => "phased",
            Self::Continuous => "continuous",
        }
    }
}

impl Architecture {
    pub fn parse(value: &str) -> Result<Self, String> {
        match value.trim() {
            "layered" => Ok(Self::Layered),
            "flat" => Ok(Self::Flat),
            other => Err(format!("architecture は layered / flat のみ: {other}")),
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Layered => "layered",
            Self::Flat => "flat",
        }
    }
}

/// 実効的な性質 = 4軸の値。束と上書きを解決した結果。
///
/// 既定はフル方法論 (prfaq + ideal-first + phased + layered)。
/// 「性質未確定の素の新規」の既定であり、kickoff が性質を確定したら束から軸を導出して上書きする。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Axes {
    pub requirements_shape: RequirementsShape,
    pub prioritization: Prioritization,
    pub delivery: Delivery,
    pub architecture: Architecture,
}

impl Default for Axes {
    fn default() -> Self {
        // 既定束 = フル方法論。
        Axes {
            requirements_shape: RequirementsShape::Prfaq,
            prioritization: Prioritization::IdealFirst,
            delivery: Delivery::Phased,
            architecture: Architecture::Layered,
        }
    }
}

impl Axes {
    /// PRFAQ 起草が active か (要件起草スキル req の起草方法を切り替える)。
    pub fn prfaq_active(self) -> bool {
        self.requirements_shape == RequirementsShape::Prfaq
    }
    /// 理想先行が active か (優先度属性 + 優先順位付けの人間ゲート)。
    pub fn ideal_first_active(self) -> bool {
        self.prioritization == Prioritization::IdealFirst
    }
    /// 段階化が active か (stage グルーピング)。
    pub fn phased_active(self) -> bool {
        self.delivery == Delivery::Phased
    }
    /// 層機構が active か (layer タグ・層別自律度勾配・quality.toml 層境界の3分解)。
    pub fn layered_active(self) -> bool {
        self.architecture == Architecture::Layered
    }

    /// 自動承認の性質既定。architecture 軸から導く
    /// (`docs/decisions/20260620-自律度根本方針と自動承認パス再設計.md`)。
    ///
    /// flat = オン (層を持たない軽量な性質。可逆操作を任せてよい)・layered = オフ (慎重側)。
    /// これは profile 由来の永続な同意源 (profile が固定の間ずっと有効)。session 由来の一時的な
    /// 引き上げ (gate.auto_enable の窓) と OR で合成し、どちらかが立てば非 guarded ゲートを auto 承認できる。
    /// 不可逆操作は層・auto と直交する独立ガード (rules.irreversible) が常に止めるので auto 対象に入らない。
    pub fn auto_approval_default(self) -> bool {
        self.architecture == Architecture::Flat
    }

    /// profile.toml の本文へ描画する (kickoff / 逆生成 draft が生成する正規形)。
    pub fn to_toml(self) -> String {
        format!(
            "[profile.axes]\nrequirements-shape = \"{}\"\nprioritization = \"{}\"\ndelivery = \"{}\"\narchitecture = \"{}\"\n",
            self.requirements_shape.as_str(),
            self.prioritization.as_str(),
            self.delivery.as_str(),
            self.architecture.as_str(),
        )
    }
}

/// 軸の一部指定。束の定義と `[profile.axes]` 上書きで共用する。
/// 省略された軸は重ねる時に触らない (基底のフル既定が残る)。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PartialAxes {
    pub requirements_shape: Option<RequirementsShape>,
    pub prioritization: Option<Prioritization>,
    pub delivery: Option<Delivery>,
    pub architecture: Option<Architecture>,
}

impl PartialAxes {
    fn from_raw(raw: &AxesRaw) -> Result<Self, String> {
        Ok(PartialAxes {
            requirements_shape: raw
                .requirements_shape
                .as_deref()
                .map(RequirementsShape::parse)
                .transpose()?,
            prioritization: raw
                .prioritization
                .as_deref()
                .map(Prioritization::parse)
                .transpose()?,
            delivery: raw.delivery.as_deref().map(Delivery::parse).transpose()?,
            architecture: raw
                .architecture
                .as_deref()
                .map(Architecture::parse)
                .transpose()?,
        })
    }

    /// 指定された軸だけ base へ重ねる。
    fn overlay(&self, base: &mut Axes) {
        if let Some(v) = self.requirements_shape {
            base.requirements_shape = v;
        }
        if let Some(v) = self.prioritization {
            base.prioritization = v;
        }
        if let Some(v) = self.delivery {
            base.delivery = v;
        }
        if let Some(v) = self.architecture {
            base.architecture = v;
        }
    }
}

/// プロジェクトの性質宣言。profile.toml の型付き表現。
///
/// 無ければ既定束 (フル方法論) として解決する (profile.toml 不在 = 素の新規)。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Profile {
    /// 名前付き束を選ぶ (主導線)。組込み or プロジェクト定義。
    pub preset: Option<String>,
    /// 束からの差分上書き (上級者のみ)。
    pub overrides: PartialAxes,
    /// このプロジェクト独自の束 (`[bundles.<name>]`・コード不要で足せる)。
    pub bundles: BTreeMap<String, PartialAxes>,
}

impl Profile {
    /// profile.toml を読む。未知の節/軸は弾く (deny_unknown_fields)。値の未知は parse が弾く。
    pub fn from_toml(text: &str) -> Result<Profile, String> {
        let raw: ProfileRaw = toml::from_str(text).map_err(|e| e.to_string())?;
        let mut bundles = BTreeMap::new();
        for (name, axes_raw) in &raw.bundles {
            bundles.insert(name.clone(), PartialAxes::from_raw(axes_raw)?);
        }
        Ok(Profile {
            preset: raw.profile.preset.filter(|s| !s.trim().is_empty()),
            overrides: PartialAxes::from_raw(&raw.profile.axes)?,
            bundles,
        })
    }

    /// 実効軸へ解決する。基底のフル既定に束を重ね、その上に上書きを重ねる。
    ///
    /// preset 未知はエラー (有効な束名を返す)。プロジェクト定義の束が組込みと同名なら前者を優先。
    pub fn resolve(&self) -> Result<Axes, String> {
        let mut axes = Axes::default();
        if let Some(name) = &self.preset {
            let bundle = self.lookup_bundle(name)?;
            bundle.overlay(&mut axes);
        }
        self.overrides.overlay(&mut axes);
        Ok(axes)
    }

    /// 束名から軸の一部指定を引く。プロジェクト定義 → 組込みの順。
    fn lookup_bundle(&self, name: &str) -> Result<PartialAxes, String> {
        if let Some(b) = self.bundles.get(name) {
            return Ok(*b);
        }
        if let Some(b) = builtin_bundle(name) {
            return Ok(b);
        }
        let mut valid: Vec<String> = builtin_bundle_names()
            .iter()
            .map(|s| s.to_string())
            .collect();
        valid.extend(self.bundles.keys().cloned());
        Err(format!(
            "preset は既知の束のみ: {name} (有効: {})",
            valid.join(" / ")
        ))
    }
}

/// 組込みの標準束名 (有効値の提示に使う)。
pub fn builtin_bundle_names() -> &'static [&'static str] {
    &[
        "clean-arch-app",
        "script",
        "library",
        "data-platform",
        "research",
    ]
}

/// 組込みの標準束 → 軸の対応 (`docs/decisions/20260618-Phase9-性質軸適応機構.md`)。
/// 全束が4軸を完全指定する (部分指定は独自束のみ)。
fn builtin_bundle(name: &str) -> Option<PartialAxes> {
    let full = |rs, pr, de, ar| {
        Some(PartialAxes {
            requirements_shape: Some(rs),
            prioritization: Some(pr),
            delivery: Some(de),
            architecture: Some(ar),
        })
    };
    use Architecture::*;
    use Delivery::*;
    use Prioritization::*;
    use RequirementsShape::*;
    match name {
        // フル方法論。
        "clean-arch-app" => full(Prfaq, IdealFirst, Phased, Layered),
        // 全 off。
        "script" => full(Lightweight, Incremental, Continuous, Flat),
        // 公開 API の契約重視・連続リリース。
        "library" => full(Prfaq, IdealFirst, Continuous, Layered),
        // パイプラインは層あり・段階整備・要件軽め。
        "data-platform" => full(Lightweight, Incremental, Phased, Layered),
        // 探索 (全 off)。
        "research" => full(Lightweight, Incremental, Continuous, Flat),
        _ => None,
    }
}

/// profile.set。性質 (preset + 任意の軸上書き) を `.owox/profile.toml` へ書き、変更を来歴へ残す。
///
/// 性質は固定だが永久ロックでなく後から変更できる (state.set と同型・監査のため来歴連動)。
/// preset / 軸値が未知なら書かずに失敗する (resolve で検証)。
pub fn set_profile(
    owox_dir: &Path,
    today: &str,
    preset: Option<String>,
    overrides: PartialAxes,
) -> Envelope {
    let preset = preset.filter(|s| !s.trim().is_empty());
    let profile = Profile {
        preset: preset.clone(),
        overrides,
        bundles: BTreeMap::new(),
    };
    // 書く前に解決して検証する (未知 preset / 軸値を弾く)。
    let axes = match profile.resolve() {
        Ok(a) => a,
        Err(err) => return Envelope::failed(err),
    };

    let mut body = String::new();
    if let Some(name) = &preset {
        body.push_str(&format!("[profile]\npreset = \"{name}\"\n\n"));
    }
    // 上書きがあれば軸節を足す (preset 無しでも実効軸を明示できる)。
    if overrides != PartialAxes::default() {
        body.push_str("[profile.axes]\n");
        if let Some(v) = overrides.requirements_shape {
            body.push_str(&format!("requirements-shape = \"{}\"\n", v.as_str()));
        }
        if let Some(v) = overrides.prioritization {
            body.push_str(&format!("prioritization = \"{}\"\n", v.as_str()));
        }
        if let Some(v) = overrides.delivery {
            body.push_str(&format!("delivery = \"{}\"\n", v.as_str()));
        }
        if let Some(v) = overrides.architecture {
            body.push_str(&format!("architecture = \"{}\"\n", v.as_str()));
        }
    }
    if body.is_empty() {
        // preset も上書きも無い = フル既定を明示する。
        body.push_str(&axes.to_toml());
    }

    if let Err(err) = std::fs::create_dir_all(owox_dir) {
        return Envelope::failed(format!("{} を作れない: {err}", owox_dir.display()));
    }
    let path = owox_dir.join("profile.toml");
    if let Err(err) = std::fs::write(&path, body) {
        return Envelope::failed(format!("{} へ書けない: {err}", path.display()));
    }

    let summary = preset.clone().unwrap_or_else(|| "custom".to_string());
    let record = record_decision(
        owox_dir,
        today,
        RecordInput {
            title: format!("Set project nature to {summary}"),
            status: DecisionStatus::Adopted,
            rationale: format!(
                "Project nature declared (requirements-shape={}, prioritization={}, delivery={}, architecture={}). This turns the development methodology modules on or off.",
                axes.requirements_shape.as_str(),
                axes.prioritization.as_str(),
                axes.delivery.as_str(),
                axes.architecture.as_str(),
            ),
            links: DecisionLinks::default(),
            supersedes: Vec::new(),
        },
    );
    let decision_ids = record.decision_ids;

    Envelope::ok(
        format!("Project nature set to {summary}."),
        json!({
            "preset": preset,
            "axes": {
                "requirements-shape": axes.requirements_shape.as_str(),
                "prioritization": axes.prioritization.as_str(),
                "delivery": axes.delivery.as_str(),
                "architecture": axes.architecture.as_str(),
            }
        }),
    )
    .with_decision_ids(decision_ids)
}

// --- 逆生成 (既存コードからの性質検出) ---

/// 逆生成の入力シグナル。core は git/走査を持たないため mcp が安く集めて渡す
/// (today / known_checks と同じく外から受ける。`docs/decisions/20260618-Phase9-性質軸適応機構.md`)。
pub struct DetectSignals<'a> {
    /// repo 相対のファイルパス一覧 (git ls-files 相当)。
    pub files: &'a [String],
    /// quality.toml が層境界か層別自律度を宣言しているか。
    pub has_quality_layers: bool,
    /// git のバージョンタグが 1 つ以上あるか。
    pub has_version_tags: bool,
}

/// 1 軸の推定 = 寄り (lean) + 根拠 (evidence)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AxisLean {
    pub value: String,
    pub evidence: String,
}

/// 逆生成の出力 = 4軸の寄りと根拠 + 最も近い組込み束。draft であり確定しない。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileDraft {
    pub requirements_shape: AxisLean,
    pub prioritization: AxisLean,
    pub delivery: AxisLean,
    pub architecture: AxisLean,
    /// 4軸の寄りに完全一致する組込み束 (あれば)。kickoff の主導線に使う。
    pub suggested_preset: Option<String>,
}

/// path のいずれかが substr を含むか (小文字比較・安いシグナル)。
fn any_contains(files: &[String], substrs: &[&str]) -> bool {
    files.iter().any(|f| {
        let lower = f.to_lowercase();
        substrs.iter().any(|s| lower.contains(s))
    })
}

/// 既存コードから性質の寄りを推定する (厚い検出)。安い言語非依存シグナルのみ。
///
/// architecture は層ディレクトリ/quality.toml 層で、プロセス3軸は docs/git のシグナルで寄せる。
/// 出力は draft + 根拠で、確定は kickoff の人間ゲートが担う (自動確定しない)。
pub fn detect_profile(sig: &DetectSignals) -> ProfileDraft {
    // architecture: 層ディレクトリか quality.toml 層宣言があれば layered。
    let layer_dirs = [
        "/domain/",
        "/usecase/",
        "/usecases/",
        "/infra/",
        "/infrastructure/",
        "/adapters/",
        "/ports/",
        "/entities/",
    ];
    let architecture = if sig.has_quality_layers {
        AxisLean {
            value: Architecture::Layered.as_str().to_string(),
            evidence: "quality.toml declares layer boundaries or autonomy".to_string(),
        }
    } else if any_contains(sig.files, &layer_dirs) {
        AxisLean {
            value: Architecture::Layered.as_str().to_string(),
            evidence: "layered directories (domain/usecase/infra/adapters/ports) are present"
                .to_string(),
        }
    } else {
        AxisLean {
            value: Architecture::Flat.as_str().to_string(),
            evidence: "no layered directory structure detected".to_string(),
        }
    };

    // requirements-shape: 要件/設計文書があれば prfaq 寄り。
    let req_docs = [
        "docs/requirements",
        "prfaq",
        "docs/rfc",
        "rfcs/",
        "docs/decisions",
        "docs/design",
    ];
    let requirements_shape = if any_contains(sig.files, &req_docs) {
        AxisLean {
            value: RequirementsShape::Prfaq.as_str().to_string(),
            evidence: "requirement / design / RFC documents are present".to_string(),
        }
    } else {
        AxisLean {
            value: RequirementsShape::Lightweight.as_str().to_string(),
            evidence: "no structured requirement documents detected".to_string(),
        }
    };

    // prioritization: ロードマップ/Vision 文書があれば ideal-first 寄り。
    let roadmap_docs = ["roadmap", "vision", "docs/roadmap"];
    let prioritization = if any_contains(sig.files, &roadmap_docs) {
        AxisLean {
            value: Prioritization::IdealFirst.as_str().to_string(),
            evidence: "roadmap / vision documents suggest priority-ordered planning".to_string(),
        }
    } else {
        AxisLean {
            value: Prioritization::Incremental.as_str().to_string(),
            evidence: "no roadmap / vision documents detected".to_string(),
        }
    };

    // delivery: CHANGELOG かバージョンタグがあれば phased 寄り。
    let delivery = if sig.has_version_tags || any_contains(sig.files, &["changelog"]) {
        AxisLean {
            value: Delivery::Phased.as_str().to_string(),
            evidence: "version tags or a CHANGELOG suggest phased releases".to_string(),
        }
    } else {
        AxisLean {
            value: Delivery::Continuous.as_str().to_string(),
            evidence: "no version tags or CHANGELOG detected (trunk-based)".to_string(),
        }
    };

    // 4軸の寄りに完全一致する組込み束を探す。
    let leaned = Axes {
        requirements_shape: RequirementsShape::parse(&requirements_shape.value).unwrap(),
        prioritization: Prioritization::parse(&prioritization.value).unwrap(),
        delivery: Delivery::parse(&delivery.value).unwrap(),
        architecture: Architecture::parse(&architecture.value).unwrap(),
    };
    let suggested_preset = builtin_bundle_names()
        .iter()
        .find(|name| {
            builtin_bundle(name)
                .map(|b| {
                    let mut a = Axes::default();
                    b.overlay(&mut a);
                    a == leaned
                })
                .unwrap_or(false)
        })
        .map(|s| s.to_string());

    ProfileDraft {
        requirements_shape,
        prioritization,
        delivery,
        architecture,
        suggested_preset,
    }
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct ProfileRaw {
    #[serde(default)]
    profile: ProfileSectionRaw,
    #[serde(default)]
    bundles: BTreeMap<String, AxesRaw>,
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct ProfileSectionRaw {
    preset: Option<String>,
    #[serde(default)]
    axes: AxesRaw,
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct AxesRaw {
    #[serde(rename = "requirements-shape")]
    requirements_shape: Option<String>,
    prioritization: Option<String>,
    delivery: Option<String>,
    architecture: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_resolves_to_full() {
        let p = Profile::default();
        let axes = p.resolve().unwrap();
        assert_eq!(axes, Axes::default());
        assert!(axes.prfaq_active());
        assert!(axes.ideal_first_active());
        assert!(axes.phased_active());
        assert!(axes.layered_active());
    }

    #[test]
    fn no_file_text_is_full() {
        let p = Profile::from_toml("").unwrap();
        assert_eq!(p.resolve().unwrap(), Axes::default());
    }

    #[test]
    fn auto_default_follows_architecture() {
        // flat = オン・layered = オフ。
        let flat = Profile::from_toml("[profile]\npreset = \"script\"\n")
            .unwrap()
            .resolve()
            .unwrap();
        assert!(flat.auto_approval_default());
        let layered = Profile::from_toml("[profile]\npreset = \"clean-arch-app\"\n")
            .unwrap()
            .resolve()
            .unwrap();
        assert!(!layered.auto_approval_default());
        // 素の新規 (既定 layered) は慎重側 = オフ。
        assert!(!Axes::default().auto_approval_default());
    }

    #[test]
    fn script_preset_is_all_off() {
        let p = Profile::from_toml("[profile]\npreset = \"script\"\n").unwrap();
        let axes = p.resolve().unwrap();
        assert!(!axes.prfaq_active());
        assert!(!axes.ideal_first_active());
        assert!(!axes.phased_active());
        assert!(!axes.layered_active());
    }

    #[test]
    fn library_preset_mapping() {
        let p = Profile::from_toml("[profile]\npreset = \"library\"\n").unwrap();
        let axes = p.resolve().unwrap();
        assert_eq!(axes.requirements_shape, RequirementsShape::Prfaq);
        assert_eq!(axes.prioritization, Prioritization::IdealFirst);
        assert_eq!(axes.delivery, Delivery::Continuous);
        assert_eq!(axes.architecture, Architecture::Layered);
    }

    #[test]
    fn axes_override_on_top_of_preset() {
        // script 束 (全 off) に architecture だけ layered を上書き。
        let text = "[profile]\npreset = \"script\"\n\n[profile.axes]\narchitecture = \"layered\"\n";
        let p = Profile::from_toml(text).unwrap();
        let axes = p.resolve().unwrap();
        assert!(!axes.prfaq_active());
        assert!(axes.layered_active());
    }

    #[test]
    fn override_without_preset_on_full_base() {
        // preset 無し = フル基底。delivery だけ continuous へ。
        let p = Profile::from_toml("[profile.axes]\ndelivery = \"continuous\"\n").unwrap();
        let axes = p.resolve().unwrap();
        assert!(axes.prfaq_active());
        assert!(!axes.phased_active());
    }

    #[test]
    fn custom_bundle_partial_keeps_full_for_unset() {
        // 独自束は一部指定可。未指定軸はフル既定が残る。
        let text =
            "[profile]\npreset = \"my-thing\"\n\n[bundles.my-thing]\narchitecture = \"flat\"\n";
        let p = Profile::from_toml(text).unwrap();
        let axes = p.resolve().unwrap();
        assert!(axes.prfaq_active()); // 未指定 → フル既定
        assert!(!axes.layered_active()); // flat 指定
    }

    #[test]
    fn custom_bundle_overrides_builtin_same_name() {
        // 同名なら独自束が組込みに勝つ。
        let text =
            "[profile]\npreset = \"script\"\n\n[bundles.script]\narchitecture = \"layered\"\n";
        let p = Profile::from_toml(text).unwrap();
        let axes = p.resolve().unwrap();
        // 独自 script は architecture=layered のみ指定 → 他はフル既定。
        assert!(axes.prfaq_active());
        assert!(axes.layered_active());
    }

    #[test]
    fn unknown_preset_errors_with_valid_list() {
        let p = Profile::from_toml("[profile]\npreset = \"bogus\"\n").unwrap();
        let err = p.resolve().unwrap_err();
        assert!(err.contains("clean-arch-app"));
        assert!(err.contains("bogus"));
    }

    #[test]
    fn unknown_axis_key_rejected() {
        let err = Profile::from_toml("[profile.axes]\nbogus = \"x\"\n").unwrap_err();
        assert!(err.contains("bogus") || err.contains("unknown"));
    }

    #[test]
    fn unknown_axis_value_rejected() {
        let err = Profile::from_toml("[profile.axes]\narchitecture = \"hexagonal\"\n").unwrap_err();
        assert!(err.contains("layered") && err.contains("flat"));
    }

    #[test]
    fn unknown_top_section_rejected() {
        let err = Profile::from_toml("[bogus]\nx = 1\n").unwrap_err();
        assert!(err.contains("bogus") || err.contains("unknown"));
    }

    #[test]
    fn set_profile_writes_and_resolves() {
        use std::sync::atomic::{AtomicU32, Ordering};
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("owox-profile-set-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let env = set_profile(
            &dir,
            "20260618",
            Some("library".to_string()),
            PartialAxes::default(),
        );
        assert_eq!(env.status, crate::envelope::Status::Ok);
        // 来歴が残る。
        assert_eq!(env.decision_ids.len(), 1);
        // profile.toml が読み戻せる。
        let text = std::fs::read_to_string(dir.join("profile.toml")).unwrap();
        let axes = Profile::from_toml(&text).unwrap().resolve().unwrap();
        assert_eq!(axes.delivery, Delivery::Continuous); // library 束
        assert!(axes.layered_active());
    }

    #[test]
    fn set_profile_rejects_unknown_preset() {
        let dir = std::env::temp_dir().join(format!("owox-profile-bad-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let env = set_profile(
            &dir,
            "20260618",
            Some("bogus".to_string()),
            PartialAxes::default(),
        );
        assert_eq!(env.status, crate::envelope::Status::Failed);
        // 書かない。
        assert!(!dir.join("profile.toml").exists());
    }

    #[test]
    fn detect_layered_app_from_dirs_and_docs() {
        let files = vec![
            "src/domain/user.rs".to_string(),
            "src/infra/db.rs".to_string(),
            "docs/requirements/r1.md".to_string(),
            "docs/roadmap/plan.md".to_string(),
            "CHANGELOG.md".to_string(),
        ];
        let sig = DetectSignals {
            files: &files,
            has_quality_layers: false,
            has_version_tags: false,
        };
        let d = detect_profile(&sig);
        assert_eq!(d.architecture.value, "layered");
        assert_eq!(d.requirements_shape.value, "prfaq");
        assert_eq!(d.prioritization.value, "ideal-first");
        assert_eq!(d.delivery.value, "phased");
        // フル方法論束に一致。
        assert_eq!(d.suggested_preset.as_deref(), Some("clean-arch-app"));
    }

    #[test]
    fn detect_flat_script() {
        let files = vec!["main.py".to_string(), "util.py".to_string()];
        let sig = DetectSignals {
            files: &files,
            has_quality_layers: false,
            has_version_tags: false,
        };
        let d = detect_profile(&sig);
        assert_eq!(d.architecture.value, "flat");
        assert_eq!(d.requirements_shape.value, "lightweight");
        assert_eq!(d.delivery.value, "continuous");
        // 全 off 束 (script / research のいずれか) に一致する。
        assert!(matches!(
            d.suggested_preset.as_deref(),
            Some("script") | Some("research")
        ));
    }

    #[test]
    fn detect_layered_via_quality_and_tags() {
        let files = vec!["lib/thing.rs".to_string()];
        let sig = DetectSignals {
            files: &files,
            has_quality_layers: true,
            has_version_tags: true,
        };
        let d = detect_profile(&sig);
        assert_eq!(d.architecture.value, "layered");
        assert!(d.architecture.evidence.contains("quality.toml"));
        assert_eq!(d.delivery.value, "phased");
    }

    #[test]
    fn axes_to_toml_roundtrip() {
        let axes = Axes {
            requirements_shape: RequirementsShape::Lightweight,
            prioritization: Prioritization::Incremental,
            delivery: Delivery::Continuous,
            architecture: Architecture::Flat,
        };
        let text = axes.to_toml();
        let p = Profile::from_toml(&text).unwrap();
        assert_eq!(p.resolve().unwrap(), axes);
    }
}
