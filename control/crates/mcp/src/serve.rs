//! owox serve: MCP サーバ。Codex の MCP server (stdio) として登録され常駐する。
//!
//! 読み・計算・記録を全て tool で出す。resource は撤去した (実機 Codex でモデルが resource を
//! 取りに来ず入口 skill が破綻したため。`docs/decisions/20260613-Phase5-実機検証の是正.md`)。
//!
//! 読み (ナビ・計算): context (作業→読む先の地図) ・next (未決 = status=open の来歴 + ready タスク)。
//! 描画した本文をテキストで返す (封筒でなく content)。
//! 記録・操作: decision.record / gate.list / gate.approve / verify.run / task.* / skill.* / glossary.*。
//! これらは共通返り値 (封筒) で返す (`docs/decisions/20260613-Phase4-tool記録層.md`)。
//!
//! generate / hook は同期。serve だけ tokio runtime を起こす (rmcp が非同期前提)。

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::{
    ErrorData as McpError, ServerHandler, ServiceExt,
    model::{CallToolResult, Content, ServerCapabilities, ServerInfo},
    tool, tool_handler, tool_router,
    transport::stdio,
};
use schemars::JsonSchema;
use serde::Deserialize;

use owox_core::{
    CreateRequirementInput, CreateTaskInput, CriterionInput, DecayFinding, DecisionStatus, Dep,
    DepKind, Envelope, Phase, RecordInput, RequirementKind, RequirementStatus, Task, TaskLinks,
    UpdateRequirementInput, UpdateTaskInput, add_criterion, add_note, approve_gate,
    approve_gate_auto, close_task, create_requirement, create_task, drop_task, get_requirement,
    glossary_lookup, is_ready, link_task, link_verification, list_decisions, list_gates,
    list_requirements, list_requirements_envelope, list_skills_envelope, list_tasks,
    list_tasks_envelope, promote_skill, record_decision, register_skill, remember,
    review_lenses_envelope, run_code_decay, run_decay, run_quality, run_verify, set_state,
    update_requirement, update_task,
};

use crate::clock::today_utc;

/// owox MCP サーバ。正本 `.owox/` を読み、記録層へ書き、計算結果を tool で返す。
#[derive(Clone)]
struct OwoxServer {
    /// 正本・記録ディレクトリ `.owox/`。サーバ起動時の作業ディレクトリ基準。
    owox_dir: PathBuf,
    /// tool 一覧 (`#[tool_router]` が生成)。
    tool_router: ToolRouter<OwoxServer>,
}

/// decision.record の引数。
#[derive(Debug, Deserialize, JsonSchema)]
struct DecisionRecordParams {
    /// 判断のタイトル。1 行で書く。
    title: String,
    /// 状態: open / adopted / rejected / superseded。open は未決の人間ゲート。
    status: String,
    /// なぜそう決めたか。
    #[serde(default)]
    rationale: Option<String>,
    /// link 先。
    #[serde(default)]
    links: LinksParam,
    /// 置き換える過去の判断 ID。
    #[serde(default)]
    supersedes: Vec<String>,
    /// guarded 層で操作前ゲートに止められた時、人間承認後に触れるようにする repo 相対パス。
    /// 人間が gate.approve すると、列挙したパスへの削除・契約面編集を操作前ゲートが 1 回だけ通す。
    #[serde(default)]
    authorizes: Vec<String>,
}

/// 来歴の link 先。
#[derive(Debug, Default, Deserialize, JsonSchema)]
struct LinksParam {
    requirement: Option<String>,
    work: Option<String>,
    verification: Option<String>,
}

/// state.set の引数。
#[derive(Debug, Deserialize, JsonSchema)]
struct StateSetParams {
    /// 状態: initial / stable / maintenance。
    phase: String,
}

/// mission.start の引数。
#[derive(Debug, Deserialize, JsonSchema)]
struct MissionStartParams {
    /// 任務種別: work / kickoff / review / verify / handoff。
    #[serde(rename = "type")]
    mission_type: String,
}

/// context の引数。scope 省略時は既存の context map。
#[derive(Debug, Default, Deserialize, JsonSchema)]
struct ContextParams {
    #[serde(default)]
    scope: Option<String>,
    #[serde(default)]
    reference_id: Option<String>,
}

/// release.check の引数。
#[derive(Debug, Default, Deserialize, JsonSchema)]
struct ReleaseCheckParams {
    /// 成果物を探す dist ディレクトリ (target repo ルートからの相対)。既定はルート。
    #[serde(default)]
    dist: Option<String>,
}

/// profile.set の引数。性質 (固定) を宣言する。
#[derive(Debug, Default, Deserialize, JsonSchema)]
struct ProfileSetParams {
    /// 名前付き束: clean-arch-app / script / library / data-platform / research。主導線になる。
    #[serde(default)]
    preset: Option<String>,
    /// 軸を 1 つずつ上書きする上級者向けの口。preset の値へ重ねる。
    #[serde(default, rename = "requirements-shape")]
    requirements_shape: Option<String>,
    #[serde(default)]
    prioritization: Option<String>,
    #[serde(default)]
    delivery: Option<String>,
    #[serde(default)]
    architecture: Option<String>,
}

/// branch.note の引数。
#[derive(Debug, Deserialize, JsonSchema)]
struct BranchNoteParams {
    /// このブランチの作業に関する一時メモ。途中の判断やスクラッチ用で来歴ではない。
    text: String,
}

/// タスクの link 先。
#[derive(Debug, Default, Deserialize, JsonSchema)]
struct TaskLinksParam {
    requirement: Option<String>,
    decision: Option<String>,
    verification: Option<String>,
}

impl From<TaskLinksParam> for TaskLinks {
    fn from(p: TaskLinksParam) -> TaskLinks {
        TaskLinks {
            requirement: p.requirement,
            decision: p.decision,
            verification: p.verification,
        }
    }
}

/// 依存 1 件。kind は blocks / parent-child / related / discovered-from。
#[derive(Debug, Deserialize, JsonSchema)]
struct DepParam {
    kind: String,
    target: String,
}

impl DepParam {
    fn into_dep(self) -> Result<Dep, String> {
        Ok(Dep {
            kind: DepKind::parse(&self.kind)?,
            target: self.target,
        })
    }
}

/// 複数の依存を core の Dep へ。1 つでも不正なら Err。
fn parse_deps(deps: Vec<DepParam>) -> Result<Vec<Dep>, String> {
    deps.into_iter().map(DepParam::into_dep).collect()
}

/// 外部対応付け文字列 (`システム: 参照`) を ExternalRef へ。system が空の要素は弾く。
fn parse_external(items: Vec<String>) -> Result<Vec<owox_core::ExternalRef>, String> {
    let mut out = Vec::new();
    for item in items {
        // 最初のコロンで system と reference を分ける (reference にコロンが含まれても安全)。
        let (system, reference) = match item.split_once(':') {
            Some((s, r)) => (s.trim().to_string(), r.trim().to_string()),
            None => (item.trim().to_string(), String::new()),
        };
        if system.is_empty() {
            return Err(format!("external は \"system: reference\" で渡す: {item}"));
        }
        out.push(owox_core::ExternalRef { system, reference });
    }
    Ok(out)
}

/// profile.set の軸文字列を PartialAxes へ検証する。未知値は弾く (core の各 parse へ委譲)。
fn parse_partial_axes(p: &ProfileSetParams) -> Result<owox_core::PartialAxes, String> {
    Ok(owox_core::PartialAxes {
        requirements_shape: p
            .requirements_shape
            .as_deref()
            .map(owox_core::RequirementsShape::parse)
            .transpose()?,
        prioritization: p
            .prioritization
            .as_deref()
            .map(owox_core::Prioritization::parse)
            .transpose()?,
        delivery: p
            .delivery
            .as_deref()
            .map(owox_core::Delivery::parse)
            .transpose()?,
        architecture: p
            .architecture
            .as_deref()
            .map(owox_core::Architecture::parse)
            .transpose()?,
    })
}

/// 現在のブランチ名。detached HEAD・git 無しは "(detached)" を返す (退避キー)。
fn git_current_branch(work_dir: &Path) -> String {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(work_dir)
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .output();
    match out {
        Ok(o) if o.status.success() => {
            let name = String::from_utf8_lossy(&o.stdout).trim().to_string();
            if name.is_empty() || name == "HEAD" {
                "(detached)".to_string()
            } else {
                name
            }
        }
        _ => "(detached)".to_string(),
    }
}

/// 既存のローカルブランチ名一覧。git 無し・失敗時は空 (孤児判定を飛ばす・安全側)。
fn git_branch_list(work_dir: &Path) -> Vec<String> {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(work_dir)
        .args(["branch", "--format=%(refname:short)"])
        .output();
    match out {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout)
            .lines()
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty())
            .collect(),
        _ => Vec::new(),
    }
}

/// worktree 横断で同じバケツを引くための本体 repo の `.owox/work`。
/// git-common-dir の親を本体 repo ルートとする。git 無しは self の owox_dir/work へ退避。
fn branch_work_root(work_dir: &Path, owox_dir: &Path) -> PathBuf {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(work_dir)
        .args(["rev-parse", "--git-common-dir"])
        .output();
    if let Ok(o) = out
        && o.status.success()
    {
        let raw = String::from_utf8_lossy(&o.stdout).trim().to_string();
        if !raw.is_empty() {
            let common = {
                let p = PathBuf::from(&raw);
                if p.is_absolute() { p } else { work_dir.join(p) }
            };
            if let Some(main_root) = common.parent() {
                return main_root.join(".owox").join("work");
            }
        }
    }
    owox_dir.join("work")
}

/// git にバージョンタグが 1 つ以上あるか (逆生成の delivery シグナル)。git 無し・失敗時は false。
fn git_has_version_tags(work_dir: &Path) -> bool {
    std::process::Command::new("git")
        .arg("-C")
        .arg(work_dir)
        .args(["tag", "--list"])
        .output()
        .map(|out| out.status.success() && !out.stdout.iter().all(u8::is_ascii_whitespace))
        .unwrap_or(false)
}

/// task.create の引数。
#[derive(Debug, Deserialize, JsonSchema)]
struct TaskCreateParams {
    /// やることのタイトル。1 行で書く。
    title: String,
    #[serde(default)]
    links: TaskLinksParam,
    /// 依存。
    #[serde(default)]
    deps: Vec<DepParam>,
    /// 層タグ。層別報告に使い architecture=layered の時のみ意味を持つ。
    #[serde(default)]
    layer: Option<String>,
    /// 段タグ。stage グルーピングに使い delivery=phased の時のみ意味を持つ。
    #[serde(default)]
    stage: Option<String>,
    /// 外部システムへの対応付け。各要素は `システム: 参照` (例: `github: owner/repo#123`)。
    #[serde(default)]
    external: Vec<String>,
}

/// task.list の引数。
#[derive(Debug, Default, Deserialize, JsonSchema)]
struct TaskListParams {
    /// true なら前提解決済の ready のみ。
    #[serde(default)]
    ready: bool,
    /// 状態で絞る: todo / doing / done / blocked / dropped。
    #[serde(default)]
    status: Option<String>,
}

/// task.update の引数。
#[derive(Debug, Deserialize, JsonSchema)]
struct TaskUpdateParams {
    id: String,
    /// 新しいタイトル。内容変更なので変えるなら reason 必須・来歴へ記録する。
    #[serde(default)]
    title: Option<String>,
    /// title を変える理由。title 変更時は必須。
    #[serde(default)]
    reason: Option<String>,
    /// 新しい状態。done は task.close を使う。
    #[serde(default)]
    status: Option<String>,
    /// link を差し替える。
    #[serde(default)]
    links: Option<TaskLinksParam>,
    /// 追加する依存。
    #[serde(default)]
    deps: Vec<DepParam>,
    /// 層タグ。空文字で消す。
    #[serde(default)]
    layer: Option<String>,
    /// 段タグ。空文字で消す。
    #[serde(default)]
    stage: Option<String>,
    /// 追加する外部対応付け。各要素は `システム: 参照`。既存と重複する組は足さない。
    #[serde(default)]
    external: Vec<String>,
}

/// task.note の引数。
#[derive(Debug, Deserialize, JsonSchema)]
struct TaskNoteParams {
    id: String,
    /// 追記する一時メモ。作業状態の覚書で来歴ではない。
    text: String,
}

/// task.close の引数。
#[derive(Debug, Deserialize, JsonSchema)]
struct TaskCloseParams {
    id: String,
}

/// task.drop の引数。
#[derive(Debug, Deserialize, JsonSchema)]
struct TaskDropParams {
    id: String,
    /// 破棄する理由。来歴へ残すため必須。
    reason: String,
}

/// task.link の引数。
#[derive(Debug, Deserialize, JsonSchema)]
struct TaskLinkParams {
    id: String,
    dep: DepParam,
}

/// skill.register / skill.promote の引数 (どちらも skill id だけ)。
#[derive(Debug, Deserialize, JsonSchema)]
struct SkillIdParams {
    /// 対象スキルの id。`.owox/skills/<id>/` を指す。
    id: String,
}

/// skill.remember の引数。
#[derive(Debug, Deserialize, JsonSchema)]
struct SkillRememberParams {
    /// 対象スキルの id。
    id: String,
    /// 追記する経験。memory.md へ書く再生成では出さない正本側の学び。
    text: String,
}

/// canon.add の引数。
#[derive(Debug, Deserialize, JsonSchema)]
struct CanonAddParams {
    /// 追加先: brand / rules / practices / glossary。
    target: String,
    /// brand / rules でどの一覧へ足すかの見出し。glossary / practices では不要。
    #[serde(default)]
    section: Option<String>,
    /// 追加する内容。glossary は "用語: 定義"・practices は指針・brand/rules は項目。
    text: String,
}

/// canon.propose の引数。
#[derive(Debug, Deserialize, JsonSchema)]
struct CanonProposeParams {
    /// 対象: brand / rules / practices / glossary。
    target: String,
    /// 構造化変更: remove は item を削除、replace は item を to へ置換。承認時に owox が適用する。
    #[serde(default)]
    op: Option<String>,
    /// brand / rules でどの一覧かの見出し。glossary / practices では不要。
    #[serde(default)]
    section: Option<String>,
    /// remove / replace で対象にする既存項目のテキスト。
    #[serde(default)]
    item: Option<String>,
    /// replace の置換後テキスト。
    #[serde(default)]
    to: Option<String>,
    /// op を使わない自由文の提案。単純な 1 項目でない編集の逃げ道。
    #[serde(default)]
    change: Option<String>,
}

/// experience.export の引数。
#[derive(Debug, Deserialize, JsonSchema)]
struct ExperienceExportParams {
    /// 経験束を書き出す先のパス。
    out_path: String,
}

/// experience.import の引数。
#[derive(Debug, Deserialize, JsonSchema)]
struct ExperienceImportParams {
    /// 経験束を読む元のパス。
    in_path: String,
}

/// glossary.lookup の引数 (用語名だけ)。
#[derive(Debug, Deserialize, JsonSchema)]
struct GlossaryTermParams {
    /// 用語名。
    term: String,
}

/// practice.lookup の引数。
#[derive(Debug, Default, Deserialize, JsonSchema)]
struct PracticeLookupParams {
    /// 探す語。指針の本文に部分一致する。空なら全件を新しい順で返す。
    #[serde(default)]
    query: String,
}

/// knowledge.add の引数。
#[derive(Debug, Deserialize, JsonSchema)]
struct KnowledgeAddParams {
    /// 調査の主題。1 行で書く。
    title: String,
    /// 調査結果の要約。散文で書く。
    summary: String,
    /// 出典の並び。URL か参照で書く。
    #[serde(default)]
    sources: Vec<String>,
    /// 調査日。YYYYMMDD 形式で、省略時は今日。
    #[serde(default)]
    researched_on: Option<String>,
    /// lookup を助けるタグ。
    #[serde(default)]
    tags: Vec<String>,
    /// 置き換える旧調査の ID。指定すると旧を superseded にし、新を current で書く。
    #[serde(default)]
    supersedes: Vec<String>,
}

/// knowledge.list の引数。
#[derive(Debug, Default, Deserialize, JsonSchema)]
struct KnowledgeListParams {
    /// 状態で絞る: current / superseded。
    #[serde(default)]
    status: Option<String>,
    /// true なら鮮度切れ、つまり current かつ調査日から閾値超えのみ。
    #[serde(default)]
    stale: bool,
}

/// knowledge.get の引数。
#[derive(Debug, Deserialize, JsonSchema)]
struct KnowledgeGetParams {
    /// 調査知識の ID。
    id: String,
}

/// knowledge.lookup の引数。
#[derive(Debug, Deserialize, JsonSchema)]
struct KnowledgeLookupParams {
    /// 探す語。title / summary / tags に部分一致する。
    query: String,
}

/// gate.approve の引数。
#[derive(Debug, Deserialize, JsonSchema)]
struct GateApproveParams {
    /// 承認する来歴 ID。
    id: String,
    /// 承認注記。
    #[serde(default)]
    note: Option<String>,
}

/// 来歴 ID だけを受ける引数 (gate.auto_approve / gate.confirm / gate.revert)。
#[derive(Debug, Deserialize, JsonSchema)]
struct GateIdParams {
    /// 対象の来歴 ID。
    id: String,
}

/// correction.note の引数。
#[derive(Debug, Deserialize, JsonSchema)]
struct CorrectionNoteParams {
    /// 守るべき指針を 1 文で。practices へ載る候補の本文になる。
    lesson: String,
    /// 何を訂正されたかの短い説明。
    #[serde(default)]
    summary: Option<String>,
}

/// 受け入れ基準 1 件 (requirement.create の inline 入力)。
#[derive(Debug, Deserialize, JsonSchema)]
struct CriterionParam {
    given: String,
    when: String,
    then: String,
    /// 検証 link。config.toml の [[verify.checks]] の検査名で未知名は弾かれる。後から link_verification でも張れる。
    #[serde(default)]
    verify: Option<String>,
}

impl From<CriterionParam> for CriterionInput {
    fn from(p: CriterionParam) -> CriterionInput {
        CriterionInput {
            given: p.given,
            when: p.when,
            then: p.then,
            verify: p.verify,
        }
    }
}

/// 理想先行 (prioritization=ideal-first) では優先度ランクは人間の判断。
/// AI が起草時に優先度を付けたら弾く。profile 未解決 (None) は素通り (安全側)。
fn ai_priority_blocked(axes: Option<&owox_core::Axes>, priority_set: bool) -> bool {
    priority_set
        && axes.is_some_and(|a| matches!(a.prioritization, owox_core::Prioritization::IdealFirst))
}

/// PRFAQ (requirements-shape=prfaq) では起草時に便益・なぜ (誰がどう得をするか) を必須にする。
/// 逆算の蒸留を行動の地点 (create) で強制し、設定が lightweight と異なる行動差を生む
/// (`docs/decisions/20260620-要件分類とPRFAQ正本.md`)。profile 未解決 (None) は素通り (安全側)。
fn prfaq_benefit_missing(axes: Option<&owox_core::Axes>, benefit_set: bool) -> bool {
    !benefit_set && axes.is_some_and(|a| a.prfaq_active())
}

/// requirement.create の引数。
#[derive(Debug, Deserialize, JsonSchema)]
struct RequirementCreateParams {
    /// 要件のタイトル。1 行で書く。
    title: String,
    /// 要件本文。何を満たすべきかを書く。
    #[serde(default)]
    statement: Option<String>,
    /// 状態: draft が既定 / accepted / superseded。
    #[serde(default)]
    status: Option<String>,
    /// 受け入れ基準。後から add_criterion でも足せる。
    #[serde(default)]
    acceptance: Vec<CriterionParam>,
    /// 置き換える旧要件 ID。
    #[serde(default)]
    supersedes: Vec<String>,
    /// 優先度ランク。小さいほど高優先で人間が並べ、prioritization=ideal-first の時のみ意味を持つ。
    #[serde(default)]
    priority: Option<u32>,
    /// 層タグ。architecture=layered の時のみ意味を持つ。
    #[serde(default)]
    layer: Option<String>,
    /// 段タグ。delivery=phased の時のみ意味を持つ。
    #[serde(default)]
    stage: Option<String>,
    /// 種類: functional / non-functional。技術・設計上の制約は要件でなく来歴へ。
    #[serde(default)]
    kind: Option<String>,
    /// 便益・なぜ。誰がどう得をするかを来歴へ記録し要件へ link する。
    /// requirements-shape=prfaq では必須。
    #[serde(default)]
    benefit: Option<String>,
}

/// requirement.list の引数。
#[derive(Debug, Default, Deserialize, JsonSchema)]
struct RequirementListParams {
    /// 状態で絞る: draft / accepted / superseded。
    #[serde(default)]
    status: Option<String>,
}

/// requirement.get の引数。
#[derive(Debug, Deserialize, JsonSchema)]
struct RequirementGetParams {
    id: String,
}

/// requirement.update の引数。
#[derive(Debug, Deserialize, JsonSchema)]
struct RequirementUpdateParams {
    id: String,
    /// 新しいタイトル。本質変更なので変えるなら reason 必須・来歴へ記録。
    #[serde(default)]
    title: Option<String>,
    /// 新しい要件本文。本質変更なので変えるなら reason 必須・来歴へ記録。
    #[serde(default)]
    statement: Option<String>,
    /// 新しい状態: draft / accepted / superseded。
    #[serde(default)]
    status: Option<String>,
    /// title / statement を変える理由。本質変更時は必須。
    #[serde(default)]
    reason: Option<String>,
    /// 優先度ランク。設定時のみ変える。
    #[serde(default)]
    priority: Option<u32>,
    /// 層タグ。空文字で消す。
    #[serde(default)]
    layer: Option<String>,
    /// 段タグ。空文字で消す。
    #[serde(default)]
    stage: Option<String>,
    /// 種類: functional / non-functional。空文字で消す。
    #[serde(default)]
    kind: Option<String>,
}

/// requirement.add_criterion の引数。
#[derive(Debug, Deserialize, JsonSchema)]
struct RequirementAddCriterionParams {
    /// 対象要件 ID。
    id: String,
    given: String,
    when: String,
    then: String,
}

/// requirement.link_verification の引数。
#[derive(Debug, Deserialize, JsonSchema)]
struct RequirementLinkVerificationParams {
    /// 対象要件 ID。
    id: String,
    /// 受け入れ基準の番号。
    criterion: u32,
    /// 検証 link。config.toml の [[verify.checks]] の検査名で未知名は弾かれる。
    verification: String,
}

impl OwoxServer {
    /// 検査・生成を実行する target repo ルート (`.owox` の親)。
    fn repo_root(&self) -> &Path {
        self.owox_dir.parent().unwrap_or(&self.owox_dir)
    }

    /// config の検査名一覧。検証 link の照合に使う。読めなければ空。
    fn known_check_names(&self) -> Vec<String> {
        owox_core::load_canon(&self.owox_dir)
            .map(|c| c.verify.checks.iter().map(|c| c.name.clone()).collect())
            .unwrap_or_default()
    }

    /// quality.toml の層名一覧。要件/タスクの layer タグ照合に使う。読めなければ空 (照合しない)。
    fn known_layer_names(&self) -> Vec<String> {
        owox_core::load_canon(&self.owox_dir)
            .map(|c| c.quality.layer_names())
            .unwrap_or_default()
    }

    /// 性質軸を解決する (profile.toml)。読めない/解決失敗時はフル方法論既定で振る舞う。
    fn resolved_axes(&self) -> owox_core::Axes {
        owox_core::load_canon(&self.owox_dir)
            .ok()
            .and_then(|canon| canon.profile.resolve().ok())
            .unwrap_or_default()
    }

    /// 逆生成シグナル (DetectSignals) の素材。profile.detect / canon.detect / kickoff で共用する。
    fn detect_inputs(&self) -> (Vec<String>, bool, bool) {
        detect_inputs(self.repo_root(), &self.owox_dir)
    }

    /// 性質が宣言済みか (profile.toml が実在)。未宣言なら kickoff が profile.detect 案を能動返却する。
    fn profile_declared(&self) -> bool {
        self.owox_dir.join("profile.toml").exists()
    }

    /// 自動承認の同意源2系統 (`docs/decisions/20260620-自律度根本方針と自動承認パス再設計.md`)。
    ///
    /// profile 由来 = architecture=flat の性質既定 (永続)・session 由来 = gate.auto_enable の窓 (session 限り)。
    fn auto_sources(&self) -> AutoApproval {
        AutoApproval {
            profile: self.resolved_axes().auto_approval_default(),
            session: crate::cache::auto_window_open(&self.owox_dir),
        }
    }

    /// 現在 session の任務。橋が無い時は既定 `work`。
    fn mission(&self) -> crate::cache::Mission {
        crate::cache::current_mission(&self.owox_dir)
    }

    /// 封筒返却へ現在任務を必ず付ける。
    fn envelope_result(&self, envelope: Envelope) -> Result<CallToolResult, McpError> {
        envelope_result(self.mission(), envelope)
    }

    /// 描画本文返却へ現在任務を必ず先頭表示する。
    fn text_result(&self, body: String) -> CallToolResult {
        CallToolResult::success(vec![Content::text(render_with_mission(
            self.mission(),
            &body,
        ))])
    }

    fn mission_data(&self, mission: crate::cache::Mission) -> Result<serde_json::Value, McpError> {
        let preview = mission_preview(&self.owox_dir, self.repo_root(), mission);
        let mut data = serde_json::json!({ "mission": mission.as_str() });
        if let Some(obj) = data.as_object_mut() {
            if let Some(preview) = preview {
                obj.insert("next_preview".to_string(), serde_json::json!(preview));
            }
        }
        if mission == crate::cache::Mission::Kickoff
            && let Ok(canon) = owox_core::load_canon(&self.owox_dir)
        {
            let decisions = list_decisions(&self.owox_dir)
                .map_err(|err| McpError::internal_error(format!("来歴を読めない: {err}"), None))?;
            let tasks = list_tasks(&self.owox_dir).map_err(|err| {
                McpError::internal_error(format!("タスクを読めない: {err}"), None)
            })?;
            if let Some(obj) = data.as_object_mut() {
                obj.insert(
                    "kickoff".to_string(),
                    kickoff_status_json(
                        &self.owox_dir,
                        self.repo_root(),
                        &canon,
                        &decisions,
                        &tasks,
                    ),
                );
            }
        }
        Ok(data)
    }
}

/// 自動承認の同意源2系統 (`docs/decisions/20260620-自律度根本方針と自動承認パス再設計.md`)。
///
/// profile 由来 = 性質既定 (architecture=flat・永続)・session 由来 = gate.auto_enable の窓 (session 限り)。
/// どちらかが立てば非 guarded ゲートを auto 承認できる。
#[derive(Clone, Copy)]
struct AutoApproval {
    profile: bool,
    session: bool,
}

impl AutoApproval {
    /// 実効的に有効か (どちらかの同意源が立つ)。
    fn active(self) -> bool {
        self.profile || self.session
    }
}

/// profile.detect の draft を JSON 値へ。profile.detect / kickoff で共用する。
fn profile_draft_value(draft: &owox_core::ProfileDraft) -> serde_json::Value {
    let lean =
        |a: &owox_core::AxisLean| serde_json::json!({ "value": a.value, "evidence": a.evidence });
    serde_json::json!({
        "suggested_preset": draft.suggested_preset,
        "axes": {
            "requirements-shape": lean(&draft.requirements_shape),
            "prioritization": lean(&draft.prioritization),
            "delivery": lean(&draft.delivery),
            "architecture": lean(&draft.architecture),
        }
    })
}

/// canon.detect の draft を JSON 値へ。canon.detect / kickoff で共用する。貼れる断片も含める。
fn canon_draft_value(draft: &owox_core::CanonDraft) -> serde_json::Value {
    let layers: Vec<_> = draft
        .layers
        .iter()
        .map(|l| serde_json::json!({ "name": l.name, "paths": l.paths, "autonomy": l.autonomy, "evidence": l.evidence }))
        .collect();
    let boundaries: Vec<_> = draft
        .boundaries
        .iter()
        .map(|b| serde_json::json!({ "paths": b.paths, "forbid": b.forbid, "reason": b.reason, "evidence": b.evidence }))
        .collect();
    let irreversible: Vec<_> = draft
        .irreversible
        .iter()
        .map(|i| serde_json::json!({ "operation": i.operation, "reason": i.reason, "detect": i.detect, "evidence": i.evidence }))
        .collect();
    serde_json::json!({
        "layers": layers,
        "boundaries": boundaries,
        "irreversible": irreversible,
        "quality_toml": owox_core::render_quality_toml(draft),
        "rules_markdown": owox_core::render_rules_markdown(draft),
    })
}

fn render_with_mission(mission: crate::cache::Mission, body: &str) -> String {
    format!("mission: {}\n\n{}", mission.as_str(), body)
}

#[tool_router(router = tool_router)]
impl OwoxServer {
    /// 判断を来歴へ記録する。status=open は未決の人間ゲートになる。
    #[tool(
        name = "decision.record",
        description = "Record a durable decision; use status=open for pending human judgment."
    )]
    async fn decision_record(
        &self,
        Parameters(p): Parameters<DecisionRecordParams>,
    ) -> Result<CallToolResult, McpError> {
        let status = match DecisionStatus::parse(&p.status) {
            Ok(s) => s,
            Err(err) => return self.envelope_result(Envelope::failed(err)),
        };
        let input = RecordInput {
            title: p.title,
            status,
            rationale: p.rationale.unwrap_or_default(),
            links: owox_core::DecisionLinks {
                requirement: p.links.requirement,
                work: p.links.work,
                verification: p.links.verification,
            },
            supersedes: p.supersedes,
        };
        // authorizes 付きは guarded 層の解凍ゲートとして記録する (承認時に層ゲートが 1 回通す)。
        if p.authorizes.is_empty() {
            self.envelope_result(record_decision(&self.owox_dir, &today_utc(), input))
        } else {
            self.envelope_result(owox_core::record_decision_with_authorization(
                &self.owox_dir,
                &today_utc(),
                input,
                p.authorizes,
            ))
        }
    }

    /// 未承認の判断点 (status=open の来歴) を一覧する。
    #[tool(name = "gate.list", description = "List pending human gates.")]
    async fn gate_list(&self) -> Result<CallToolResult, McpError> {
        self.envelope_result(list_gates(&self.owox_dir))
    }

    /// 現在 session の任務を切り替える。kickoff などの間、tool の返り方を任務向けへ寄せる。
    #[tool(
        name = "mission.start",
        description = "Switch the current session mission; use work for the default work mode."
    )]
    async fn mission_start(
        &self,
        Parameters(p): Parameters<MissionStartParams>,
    ) -> Result<CallToolResult, McpError> {
        let mission = match crate::cache::Mission::parse(&p.mission_type) {
            Ok(m) => m,
            Err(err) => return self.envelope_result(Envelope::failed(err)),
        };
        if let Err(err) = crate::cache::set_current_mission(&self.owox_dir, mission) {
            return self.envelope_result(Envelope::failed(err));
        }
        let data = self.mission_data(mission)?;
        self.envelope_result(Envelope::ok(
            format!("Session mission set to {}.", mission.as_str()),
            data,
        ))
    }

    /// 文脈ナビ。作業 → 読む先の地図を返す。canon を直読みせずここから取る。
    ///
    /// 読みは tool に一本化した (resource はモデルが取りに来ず不安定。
    /// `docs/decisions/20260613-Phase5-実機検証の是正.md`)。封筒でなく描画した本文を返す。
    #[tool(
        name = "context",
        description = "Get the current read map instead of reading .owox/ directly."
    )]
    async fn context(
        &self,
        Parameters(p): Parameters<ContextParams>,
    ) -> Result<CallToolResult, McpError> {
        if let Some(reference_id) = p.reference_id.as_deref().map(str::trim)
            && !reference_id.is_empty()
        {
            if parse_reference_query(reference_id).is_none() {
                return self.envelope_result(Envelope::failed(
                    "reference_id は owox:req:<id>, owox:req:<id>#<criterion>, owox:dec:<id> のみ",
                ));
            }
            let body = render_reference_context(
                self.repo_root(),
                &self.owox_dir,
                reference_id,
                self.mission(),
            )?;
            return Ok(self.text_result(body));
        }
        let body = match p.scope.as_deref() {
            None | Some("") | Some("default") => {
                let canon = owox_core::load_canon(&self.owox_dir).map_err(|err| {
                    McpError::internal_error(format!("正本を読めない: {err}"), None)
                })?;
                if self.mission() == crate::cache::Mission::Kickoff {
                    let decisions = list_decisions(&self.owox_dir).map_err(|err| {
                        McpError::internal_error(format!("来歴を読めない: {err}"), None)
                    })?;
                    let tasks = list_tasks(&self.owox_dir).map_err(|err| {
                        McpError::internal_error(format!("タスクを読めない: {err}"), None)
                    })?;
                    render_kickoff_context(
                        self.repo_root(),
                        &self.owox_dir,
                        &canon,
                        &decisions,
                        &tasks,
                    )
                } else {
                    render_context(&canon.context)
                }
            }
            Some("diff") => render_diff_context(self.repo_root(), &self.owox_dir, self.mission())?,
            Some("codebase") => {
                render_codebase_context(self.repo_root(), &self.owox_dir, self.mission())?
            }
            Some(other) => {
                return self.envelope_result(Envelope::failed(format!(
                    "context scope は default / diff / codebase のみ: {other}"
                )));
            }
        };
        Ok(self.text_result(body))
    }

    /// 次の一手。未決の人間ゲート (status=open の来歴) と ready タスクを返す。
    #[tool(
        name = "next",
        description = "Get the next gate or ready work, with mission-aware guidance when active."
    )]
    async fn next(&self) -> Result<CallToolResult, McpError> {
        let decisions = list_decisions(&self.owox_dir)
            .map_err(|err| McpError::internal_error(format!("来歴を読めない: {err}"), None))?;
        let tasks = list_tasks(&self.owox_dir)
            .map_err(|err| McpError::internal_error(format!("タスクを読めない: {err}"), None))?;
        let requirements = list_requirements(&self.owox_dir)
            .map_err(|err| McpError::internal_error(format!("要件を読めない: {err}"), None))?;
        let repo_root = self.repo_root();
        let canon = owox_core::load_canon(&self.owox_dir).ok();
        if self.mission() == crate::cache::Mission::Kickoff
            && let Some(canon) = canon.as_ref()
        {
            let questions =
                build_kickoff_questions(&self.owox_dir, repo_root, canon, &decisions, &tasks);
            return Ok(self.text_result(render_kickoff_next(&questions)));
        }
        // 腐敗検知の閾値は quality.toml の [decay]。正本が読めない時は警告を出さない (作業を妨げない)。
        // 成長層 (practices) の鮮度も合流する (canon 内で軽い・next の高速性を崩さない)。
        let decay = canon
            .as_ref()
            .map(|canon| {
                let mut d = run_decay(&tasks, &decisions, &canon.quality.decay, &today_utc());
                d.extend(owox_core::run_practice_decay(
                    &canon.practices.entries,
                    canon.quality.decay.review_decision_days,
                    &today_utc(),
                ));
                // 似た practice の冗長性も合流する (床注入が膨らむ前に統合を促す advisory)。
                d.extend(owox_core::run_practice_redundancy(
                    &canon.practices.entries,
                    canon.quality.decay.practice_similarity,
                ));
                // 調査知識の鮮度も合流する (件数は少ない想定。重ければ verify のみへ寄せる)。
                let knowledge = owox_core::list_knowledge(&self.owox_dir).unwrap_or_default();
                d.extend(owox_core::run_knowledge_decay(
                    &knowledge,
                    canon.quality.decay.knowledge_stale_days,
                    &today_utc(),
                ));
                // ブランチ作業記憶の鮮度・孤児も合流する (件数は少ない・next の高速性を崩さない)。
                let work_dir = self.repo_root();
                let work_root = branch_work_root(work_dir, &self.owox_dir);
                let mems = owox_core::list_branch_memories(&work_root).unwrap_or_default();
                d.extend(owox_core::run_branch_memory_decay(
                    &mems,
                    &git_branch_list(work_dir),
                    canon.quality.decay.branch_memory_stale_days,
                    &today_utc(),
                ));
                d
            })
            .unwrap_or_default();
        // 育てられる手順 (頻出する隣接 tool / コマンド列) を助言する。usage.log + skills を読む (軽量)。
        let skills = owox_core::load_skills(&self.owox_dir).unwrap_or_default();
        let routines = canon
            .as_ref()
            .map(|canon| {
                owox_core::run_routine_suggestions(&self.owox_dir, &canon.quality.routine, &skills)
            })
            .unwrap_or_default();
        let gardening = canon
            .as_ref()
            .map(|canon| {
                build_gardening_findings(&self.owox_dir, repo_root, &canon, &decay, None, false)
            })
            .unwrap_or_default();
        let changed_files = crate::files::changed_files(repo_root);
        let glossary_suggestions =
            build_glossary_suggestions(repo_root, &self.owox_dir, &changed_files, true);
        // 性質軸を解決する (profile.toml)。読めない/解決失敗時はフル方法論既定で振る舞う。
        let axes = self.resolved_axes();
        // 自動承認の同意源2系統 (profile 由来=永続・session 由来=session 限り)。
        let auto = self.auto_sources();
        Ok(self.text_result(render_next(
            &decisions,
            &tasks,
            &requirements,
            &decay,
            &routines,
            &gardening,
            &glossary_suggestions,
            axes,
            auto,
            self.mission(),
        )))
    }

    /// 判断点を承認する。open の来歴を adopted へ遷移し承認注記を残す。
    ///
    /// destructive 注釈でクライアントへ人間確認を要求する (人間ゲートを AI が無確認で素通りできない
    /// ようにする)。ただし確認を出すか否かはクライアント任せで、trust_level=trusted 等の全面信頼下では
    /// 確認が省かれ得る。人間ゲート保証は「クライアントが確認を出すこと」が前提で、生成 target 設定で
    /// ゲートツールを常時確認へ固定して担保する (`docs/decisions/20260620-Phase9-人間ゲートの確認依存.md`)。
    /// canon 変更が紐づくゲートなら、承認 = owox が canon へ適用する。
    #[tool(
        name = "gate.approve",
        annotations(destructive_hint = true),
        description = "Approve an open gate after the human decides."
    )]
    async fn gate_approve(
        &self,
        Parameters(p): Parameters<GateApproveParams>,
    ) -> Result<CallToolResult, McpError> {
        // canon 変更が紐づくなら、承認 = canon へ適用。適用に失敗したら承認しない。
        if let Err(err) = owox_core::apply_pending_canon_change(&self.owox_dir, &p.id) {
            return self.envelope_result(Envelope::failed(err));
        }
        self.envelope_result(approve_gate(
            &self.owox_dir,
            &today_utc(),
            &p.id,
            p.note.as_deref(),
        ))
    }

    /// 自動承認を今セッションのあいだ有効にする。人間が「寝てる間進めて」等と言った時に呼ぶ。
    ///
    /// destructive 注釈で必ず人間確認を出す。その確認プロンプトが人間の同意。窓はセッション限りで、
    /// 次のセッション開始で自動的に閉じる。
    #[tool(
        name = "gate.auto_enable",
        annotations(destructive_hint = true),
        description = "Enable session auto-approval for non-guarded gates; guarded gates still need gate.approve."
    )]
    async fn gate_auto_enable(&self) -> Result<CallToolResult, McpError> {
        crate::cache::open_auto_window(&self.owox_dir);
        self.envelope_result(Envelope::ok(
            "Automatic approval is on for this session. Approve non-guarded gates with gate.auto_approve; they are queued for the human to confirm or revert. It closes at the next session start.",
            serde_json::json!({ "auto_window": true }),
        ))
    }

    /// 自動承認の session 窓を閉じる。profile 由来 (flat) の auto は残る (profile.set で性質を変えるまで)。
    #[tool(
        name = "gate.auto_disable",
        description = "Close the session auto-approval window."
    )]
    async fn gate_auto_disable(&self) -> Result<CallToolResult, McpError> {
        crate::cache::close_auto_window(&self.owox_dir);
        // 窓を閉じても profile 由来 (flat) なら auto は残る。実態に合うメッセージを返す。
        let auto_profile = self.auto_sources().profile;
        let message = if auto_profile {
            "Session window closed, but automatic approval stays on by this project's flat nature. Change it with profile.set."
        } else {
            "Automatic approval is off."
        };
        self.envelope_result(Envelope::ok(
            message,
            serde_json::json!({ "auto_window": false, "auto_profile": auto_profile }),
        ))
    }

    /// 判断点を自動承認する。実効 auto が有効で、かつ guarded でないゲートだけ通る。
    ///
    /// destructive 注釈は付けない (人間確認を出さないのが auto の目的)。安全は二重の条件で守る:
    /// 実効 auto が有効なこと (profile 由来の性質既定か session 由来の窓)、対象が guarded でないこと。
    #[tool(
        name = "gate.auto_approve",
        description = "Approve a non-guarded gate only while auto-approval is on."
    )]
    async fn gate_auto_approve(
        &self,
        Parameters(p): Parameters<GateIdParams>,
    ) -> Result<CallToolResult, McpError> {
        if !self.auto_sources().active() {
            return self.envelope_result(Envelope::failed(
                "Automatic approval is off. Ask the human to approve with gate.approve, or have them turn on automatic approval with gate.auto_enable first.",
            ));
        }
        // guarded は auto 不可。適用前に判定し、固定層 canon を無確認で変えないようにする。
        let decision = match owox_core::load_decision(&self.owox_dir, &p.id) {
            Ok(d) => d,
            Err(err) => return self.envelope_result(Envelope::failed(err)),
        };
        if owox_core::gate_autonomy(&decision) == owox_core::Autonomy::Guarded {
            return self.envelope_result(Envelope::failed(format!(
                "Gate {} is guarded and only a human can approve it. Use gate.approve.",
                p.id
            )));
        }
        // 紐づく canon 変更があれば適用する (gate.approve と同じ合成)。失敗したら承認しない。
        if let Err(err) = owox_core::apply_pending_canon_change(&self.owox_dir, &p.id) {
            return self.envelope_result(Envelope::failed(err));
        }
        self.envelope_result(approve_gate_auto(&self.owox_dir, &today_utc(), &p.id))
    }

    /// 自動承認を人間が後から確認済みにする。後追いキューから外れる。
    #[tool(
        name = "gate.confirm",
        description = "Confirm a human-reviewed auto-approved decision."
    )]
    async fn gate_confirm(
        &self,
        Parameters(p): Parameters<GateIdParams>,
    ) -> Result<CallToolResult, McpError> {
        self.envelope_result(owox_core::confirm_auto_approval(
            &self.owox_dir,
            &today_utc(),
            &p.id,
        ))
    }

    /// 自動承認を差し戻す。紐づく canon 変更を逆適用し、来歴を rejected へ落とす。
    ///
    /// destructive 注釈で必ず人間確認を出す (canon を書き戻すため)。
    #[tool(
        name = "gate.revert",
        annotations(destructive_hint = true),
        description = "Undo an auto-approved decision and revert any applied canon change."
    )]
    async fn gate_revert(
        &self,
        Parameters(p): Parameters<GateIdParams>,
    ) -> Result<CallToolResult, McpError> {
        // canon を元へ戻す。失敗したら差し戻さない (来歴と canon の食い違いを作らない)。
        if let Err(err) = owox_core::revert_pending_canon_change(&self.owox_dir, &p.id) {
            return self.envelope_result(Envelope::failed(err));
        }
        self.envelope_result(owox_core::reject_decision(
            &self.owox_dir,
            &today_utc(),
            &p.id,
            None,
        ))
    }

    /// 人間の訂正から practice 草案を起草する。固定はせず open gate として積む。
    #[tool(
        name = "correction.note",
        description = "Record a human correction as a proposed practice gate."
    )]
    async fn correction_note(
        &self,
        Parameters(p): Parameters<CorrectionNoteParams>,
    ) -> Result<CallToolResult, McpError> {
        self.envelope_result(owox_core::propose_practice_from_correction(
            &self.owox_dir,
            &today_utc(),
            p.summary.as_deref().unwrap_or(""),
            &p.lesson,
        ))
    }

    /// 要件を作る。受け入れ基準をまとめて受けられる。
    #[tool(
        name = "requirement.create",
        description = "Create a requirement with status, criteria, and links."
    )]
    async fn requirement_create(
        &self,
        Parameters(p): Parameters<RequirementCreateParams>,
    ) -> Result<CallToolResult, McpError> {
        let status = match p
            .status
            .as_deref()
            .map(RequirementStatus::parse)
            .transpose()
        {
            Ok(s) => s.unwrap_or_default(),
            Err(err) => return self.envelope_result(Envelope::failed(err)),
        };
        let kind = match p.kind.as_deref().map(RequirementKind::parse).transpose() {
            Ok(k) => k,
            Err(err) => return self.envelope_result(Envelope::failed(err)),
        };
        // 理想先行では優先度の並び替えは人間の判断。AI が起草時にランクを付けるのを弾く
        // (`docs/decisions/20260620-要件分類とPRFAQ正本.md`)。profile が読めない時は素通り (安全側)。
        let axes = owox_core::load_canon(&self.owox_dir)
            .ok()
            .and_then(|c| c.profile.resolve().ok());
        if ai_priority_blocked(axes.as_ref(), p.priority.is_some()) {
            return self.envelope_result(Envelope::failed(
                "Under ideal-first prioritization, the priority ranking is a human decision. Create the requirement without priority, propose a ranking to the human, and set it with requirement.update only after they decide.",
            ));
        }
        let benefit_set = p.benefit.as_deref().is_some_and(|b| !b.trim().is_empty());
        if prfaq_benefit_missing(axes.as_ref(), benefit_set) {
            return self.envelope_result(Envelope::failed(
                "Under prfaq requirements-shape, work backwards: state who benefits and why before drafting. Pass benefit on requirement.create — it is recorded as a linked decision.",
            ));
        }
        let input = CreateRequirementInput {
            title: p.title,
            statement: p.statement.unwrap_or_default(),
            status,
            criteria: p.acceptance.into_iter().map(Into::into).collect(),
            supersedes: p.supersedes,
            priority: p.priority,
            layer: p.layer,
            stage: p.stage,
            kind,
            benefit: p.benefit,
        };
        self.envelope_result(create_requirement(
            &self.owox_dir,
            &today_utc(),
            &self.known_check_names(),
            &self.known_layer_names(),
            input,
        ))
    }

    /// 要件を一覧する。状態で絞れる。検証 link が欠ける基準数も返す。
    #[tool(
        name = "requirement.list",
        description = "List requirements and missing verification links."
    )]
    async fn requirement_list(
        &self,
        Parameters(p): Parameters<RequirementListParams>,
    ) -> Result<CallToolResult, McpError> {
        self.envelope_result(list_requirements_envelope(
            &self.owox_dir,
            p.status.as_deref(),
        ))
    }

    /// 要件 1 件を全文読む。canon を直読みせずここから取る。
    #[tool(
        name = "requirement.get",
        description = "Get one requirement with criteria and links."
    )]
    async fn requirement_get(
        &self,
        Parameters(p): Parameters<RequirementGetParams>,
    ) -> Result<CallToolResult, McpError> {
        self.envelope_result(get_requirement(&self.owox_dir, &p.id))
    }

    /// 要件の title・statement・状態を変える。本質変更は reason 必須・来歴連動。
    #[tool(
        name = "requirement.update",
        description = "Update a requirement; title or statement changes need a reason."
    )]
    async fn requirement_update(
        &self,
        Parameters(p): Parameters<RequirementUpdateParams>,
    ) -> Result<CallToolResult, McpError> {
        self.envelope_result(update_requirement(
            &self.owox_dir,
            &today_utc(),
            &p.id,
            &self.known_layer_names(),
            UpdateRequirementInput {
                title: p.title,
                statement: p.statement,
                status: p.status,
                reason: p.reason,
                priority: p.priority,
                layer: p.layer,
                stage: p.stage,
                kind: p.kind,
            },
        ))
    }

    /// 受け入れ基準を 1 件足す。番号は自動採番する。
    #[tool(
        name = "requirement.add_criterion",
        description = "Add an acceptance criterion."
    )]
    async fn requirement_add_criterion(
        &self,
        Parameters(p): Parameters<RequirementAddCriterionParams>,
    ) -> Result<CallToolResult, McpError> {
        self.envelope_result(add_criterion(
            &self.owox_dir,
            &p.id,
            &p.given,
            &p.when,
            &p.then,
        ))
    }

    /// 受け入れ基準に検証 link を張る。
    #[tool(
        name = "requirement.link_verification",
        description = "Link a verification check to a criterion."
    )]
    async fn requirement_link_verification(
        &self,
        Parameters(p): Parameters<RequirementLinkVerificationParams>,
    ) -> Result<CallToolResult, McpError> {
        self.envelope_result(link_verification(
            &self.owox_dir,
            &self.known_check_names(),
            &p.id,
            p.criterion,
            &p.verification,
        ))
    }

    /// 今の変更に適用されるレビュー観点を機械選択して返す (routable な枠組み)。
    #[tool(
        name = "review.lenses",
        description = "Select the review perspectives for the current change."
    )]
    async fn review_lenses(&self) -> Result<CallToolResult, McpError> {
        let work_dir = self.owox_dir.parent().unwrap_or(&self.owox_dir);
        let changed = crate::files::changed_files(work_dir);
        let mut env = review_lenses_envelope(&self.owox_dir, &changed);
        let mut ops = vec![owox_core::DeliveryOperation::Review];
        ops.extend(path_change_operations(&changed));
        attach_delivery_guidance(&mut env, &self.owox_dir, &ops, &changed);
        self.envelope_result(env)
    }

    /// 完了を3区別して返す。検証完了だけ機械判定、作業・要件完了は人間判断。
    #[tool(
        name = "verify.run",
        description = "Run configured checks and report completion status."
    )]
    async fn verify_run(&self) -> Result<CallToolResult, McpError> {
        let canon = match owox_core::load_canon(&self.owox_dir) {
            Ok(canon) => canon,
            Err(err) => {
                return self.envelope_result(Envelope::failed(format!("正本を読めない: {err}")));
            }
        };
        // 検査は target repo ルート (`.owox` の親) で実行する。
        let work_dir = self.owox_dir.parent().unwrap_or(&self.owox_dir);
        let requirements = match list_requirements(&self.owox_dir) {
            Ok(r) => r,
            Err(err) => {
                return self.envelope_result(Envelope::failed(format!("要件を読めない: {err}")));
            }
        };
        // 品質バーとブランド (禁止語) の違反を集める (ファイル列挙は mcp が git ls-files で行う)。
        // ブランド違反は kind="brand" で quality と同じチャネルに合流する (commit ゲートの phase 適応も共通)。
        let files = crate::files::list_repo_files(work_dir);
        let mut quality = run_quality(&canon.quality, work_dir, &files);
        quality.extend(owox_core::run_brand(
            &canon.glossary.forbidden,
            work_dir,
            &files,
        ));
        // 腐敗検知。タスク・来歴 (canon) に加え、コード/repo (重複ファイル・委譲検査) も verify.run で集める。
        // 読めない時は空 (報告のみで完了3区別は変えない)。重い repo 走査は next でなくここで行う。
        let tasks = list_tasks(&self.owox_dir).unwrap_or_default();
        let decisions = list_decisions(&self.owox_dir).unwrap_or_default();
        let mut decay = run_decay(&tasks, &decisions, &canon.quality.decay, &today_utc());
        decay.extend(run_code_decay(work_dir, &files, &canon.quality.decay));
        decay.extend(owox_core::run_practice_decay(
            &canon.practices.entries,
            canon.quality.decay.review_decision_days,
            &today_utc(),
        ));
        decay.extend(owox_core::run_practice_redundancy(
            &canon.practices.entries,
            canon.quality.decay.practice_similarity,
        ));
        let knowledge = owox_core::list_knowledge(&self.owox_dir).unwrap_or_default();
        decay.extend(owox_core::run_knowledge_decay(
            &knowledge,
            canon.quality.decay.knowledge_stale_days,
            &today_utc(),
        ));
        // ブランチ作業記憶の鮮度・孤児も報告する (advisory)。
        let work_root = branch_work_root(work_dir, &self.owox_dir);
        let mems = owox_core::list_branch_memories(&work_root).unwrap_or_default();
        decay.extend(owox_core::run_branch_memory_decay(
            &mems,
            &git_branch_list(work_dir),
            canon.quality.decay.branch_memory_stale_days,
            &today_utc(),
        ));
        let mut env = run_verify(&canon.verify, &requirements, &quality, &decay, work_dir);
        let changed = crate::files::changed_files(work_dir);
        let glossary_suggestions =
            build_glossary_suggestions(work_dir, &self.owox_dir, &changed, true);
        let mut ops = vec![owox_core::DeliveryOperation::Verify];
        ops.extend(path_change_operations(&changed));
        attach_delivery_guidance(&mut env, &self.owox_dir, &ops, &changed);
        let reference_files: Vec<crate::files::ChangedFile> =
            crate::files::list_repo_files(work_dir)
                .into_iter()
                .filter_map(|path| {
                    let kind = crate::files::classify_path(&path);
                    matches!(
                        kind,
                        crate::files::FileKind::Source | crate::files::FileKind::Docs
                    )
                    .then_some(crate::files::ChangedFile {
                        path,
                        previous_path: None,
                        status: crate::files::ChangeStatus::Modified,
                        kind,
                    })
                })
                .collect();
        let refs = scan_references(work_dir, &reference_files, &requirements, &decisions);
        merge_data(
            &mut env,
            "references",
            serde_json::json!({
                "requirement_refs": refs.summary.requirement_refs,
                "decision_refs": refs.summary.decision_refs,
                "broken_refs": refs.summary.broken_refs,
                "broken": refs.broken,
            }),
        );
        if self.mission() == crate::cache::Mission::Kickoff {
            merge_data(
                &mut env,
                "kickoff",
                kickoff_status_json(&self.owox_dir, work_dir, &canon, &decisions, &tasks),
            );
            let questions =
                build_kickoff_questions(&self.owox_dir, work_dir, &canon, &decisions, &tasks);
            if !questions.is_empty() {
                let mut next = env.next_actions.clone();
                next.insert(
                    0,
                    "Kickoff mission is still open. Resolve the next setup decision shown by next."
                        .to_string(),
                );
                env.next_actions = next;
            }
        }
        let gardening =
            build_gardening_findings(&self.owox_dir, work_dir, &canon, &decay, Some(&refs), true);
        if !gardening.is_empty() {
            merge_data(&mut env, "gardening", gardening_json(&gardening));
        }
        if refs.summary.broken_refs > 0 || gardening.iter().any(|f| f.severity == "failed") {
            env.status = owox_core::Status::Failed;
            env.reason = "Gardening found broken harness state.".to_string();
            let mut next = env.next_actions.clone();
            if refs.summary.broken_refs > 0 {
                next.insert(
                    0,
                    "Fix the broken references listed in data.references.broken.".to_string(),
                );
            }
            if gardening.iter().any(|f| f.kind == "broken-skill") {
                next.insert(
                    0,
                    "Fix the broken skills listed in data.gardening.findings.".to_string(),
                );
            }
            if gardening.iter().any(|f| f.kind == "generated-edit") {
                next.insert(
                    0,
                    "Revert direct edits to generated files and change the source instead."
                        .to_string(),
                );
            }
            env.next_actions = next;
        }
        // 今走らせた作業ツリーの署名と検査結果を覚える。Stop は署名一致で「verify.run 済み・以降
        // 変更なし」を判定し (合否は問わない)、commit ゲートは署名一致なら検査結果を再利用して
        // 検査の二重実行を避ける。
        if let Some(sig) = crate::files::tree_signature(work_dir) {
            let (verification, failed) = verify_outcome_from_envelope(&env);
            crate::cache::write_verify_record(
                &self.owox_dir,
                &crate::cache::VerifyRecord {
                    signature: sig,
                    verification,
                    failed,
                },
            );
        }
        // 育てられる手順を advisory として data へ後付けする (run_verify の署名は変えない)。
        let skills = owox_core::load_skills(&self.owox_dir).unwrap_or_default();
        let routines =
            owox_core::run_routine_suggestions(&self.owox_dir, &canon.quality.routine, &skills);
        if !glossary_suggestions.is_empty() {
            let items: Vec<serde_json::Value> = glossary_suggestions
                .iter()
                .map(|s| {
                    serde_json::json!({
                        "term": s.term,
                        "reason": s.reason,
                        "examples": s.examples,
                    })
                })
                .collect();
            merge_data(
                &mut env,
                "glossary_suggestions",
                serde_json::Value::Array(items),
            );
        }
        if !routines.is_empty() {
            let items: Vec<serde_json::Value> = routines
                .iter()
                .map(|r| {
                    serde_json::json!({
                        "sequence": r.sequence,
                        "occurrences": r.occurrences,
                        "kind": r.kind.as_str(),
                        "reasons": r.reasons,
                        "suggested_script": r.suggested_script,
                        "test_hint": r.test_hint,
                    })
                })
                .collect();
            merge_data(
                &mut env,
                "routine_suggestions",
                serde_json::Value::Array(items),
            );
        }
        self.envelope_result(env)
    }

    /// 配布前に成果物を検証する (配布運用がある対象プロジェクトだけ)。
    /// 版抽出・成果物存在・委譲検査を封筒で返す。owox 自身は hash を計算しない
    /// (`docs/decisions/20260621-Phase10-配布とrelease正本.md`)。
    #[tool(
        name = "release.check",
        description = "Verify a release before shipping."
    )]
    async fn release_check(
        &self,
        Parameters(p): Parameters<ReleaseCheckParams>,
    ) -> Result<CallToolResult, McpError> {
        let canon = match owox_core::load_canon(&self.owox_dir) {
            Ok(canon) => canon,
            Err(err) => {
                return self.envelope_result(Envelope::failed(format!("正本を読めない: {err}")));
            }
        };
        let release = &canon.release;
        // 配布運用なし。release.toml を置く対象プロジェクトだけが使う。
        if release.policy.is_empty()
            && release.version.is_none()
            && release.artifacts.is_empty()
            && release.checks.is_empty()
        {
            return self.envelope_result(Envelope::ok(
                "release.toml が無く配布運用なし。配布する対象プロジェクトだけ .owox/release.toml を置く",
                serde_json::json!({ "configured": false }),
            ));
        }

        let work_dir = self.owox_dir.parent().unwrap_or(&self.owox_dir);
        let dist_dir = match &p.dist {
            Some(d) => work_dir.join(d),
            None => work_dir.to_path_buf(),
        };

        // 版抽出。version.file を work_dir から読み、pattern の捕捉群で取り出す。
        let version = release.version.as_ref().and_then(|v| {
            std::fs::read_to_string(work_dir.join(&v.file))
                .ok()
                .and_then(|text| release.extract_version(&text))
        });
        // version 宣言済みなのに読み取れないのは配布の前提崩れ (厳しく扱う)。
        let version_unresolved = release.version.is_some() && version.is_none();

        // 成果物存在確認 (dist 内)。owox は hash を計算せず存在だけ見る。
        let present: Vec<String> = release
            .artifacts
            .iter()
            .filter(|name| dist_dir.join(name).exists())
            .cloned()
            .collect();
        let missing: Vec<String> = release
            .missing_artifacts(&present)
            .into_iter()
            .map(str::to_string)
            .collect();

        // checksum / 署名の実検証は対象プロジェクトのコマンドへ委譲する。dist で実行。
        let checks = owox_core::run_checks(&dist_dir, &release.checks);
        let failed_checks: Vec<&str> = checks
            .iter()
            .filter(|c| !c.passed)
            .map(|c| c.name.as_str())
            .collect();

        let data = serde_json::json!({
            "configured": true,
            "version": version,
            "version_unresolved": version_unresolved,
            "artifacts_present": present,
            "artifacts_missing": missing,
            "checks": checks
                .iter()
                .map(|c| serde_json::json!({ "name": c.name, "passed": c.passed, "detail": c.detail }))
                .collect::<Vec<_>>(),
            "policy": release.policy,
        });

        if version_unresolved || !missing.is_empty() || !failed_checks.is_empty() {
            let mut reasons = Vec::new();
            if version_unresolved {
                reasons.push("版を読み取れない".to_string());
            }
            if !missing.is_empty() {
                reasons.push(format!("成果物が無い: {}", missing.join(", ")));
            }
            if !failed_checks.is_empty() {
                reasons.push(format!("検査が失敗: {}", failed_checks.join(", ")));
            }
            return self.envelope_result(
                Envelope::failed(format!("配布前検証が通らない: {}", reasons.join(" / ")))
                    .with_data(data),
            );
        }

        self.envelope_result(Envelope::ok("配布前検証が通った", data))
    }

    /// プロジェクト状態 (phase) を宣言する。機械ゲートの厳しさが変わる。
    #[tool(
        name = "state.set",
        description = "Set the project phase; maintenance blocks commit on open decisions."
    )]
    async fn state_set(
        &self,
        Parameters(p): Parameters<StateSetParams>,
    ) -> Result<CallToolResult, McpError> {
        let phase = match Phase::parse(&p.phase) {
            Ok(phase) => phase,
            Err(err) => return self.envelope_result(Envelope::failed(err)),
        };
        self.envelope_result(set_state(&self.owox_dir, &today_utc(), phase))
    }

    /// プロジェクトの性質 (固定) を宣言する。開発方法論のモジュールが軸で出し入れされる。
    #[tool(
        name = "profile.set",
        description = "Set project nature by preset or axes."
    )]
    async fn profile_set(
        &self,
        Parameters(p): Parameters<ProfileSetParams>,
    ) -> Result<CallToolResult, McpError> {
        let overrides = match parse_partial_axes(&p) {
            Ok(o) => o,
            Err(err) => return self.envelope_result(Envelope::failed(err)),
        };
        self.envelope_result(owox_core::set_profile(
            &self.owox_dir,
            &today_utc(),
            p.preset,
            overrides,
        ))
    }

    /// 性質を既存コードから推定する (逆生成)。draft + 根拠を返し、確定はしない (人間ゲート)。
    #[tool(
        name = "profile.detect",
        description = "Detect a draft project nature for human review."
    )]
    async fn profile_detect(&self) -> Result<CallToolResult, McpError> {
        let (files, has_quality_layers, has_version_tags) = self.detect_inputs();
        let draft = owox_core::detect_profile(&owox_core::DetectSignals {
            files: &files,
            has_quality_layers,
            has_version_tags,
        });
        self.envelope_result(Envelope::ok(
            "Detected a draft project nature. Confirm with a human, then set it with profile.set.",
            profile_draft_value(&draft),
        ))
    }

    /// 既存コードから rules / quality の初期案を逆生成する。draft + 根拠を返し、確定しない (人間ゲート)。
    #[tool(
        name = "canon.detect",
        description = "Detect draft guardrails from existing code for human review."
    )]
    async fn canon_detect(&self) -> Result<CallToolResult, McpError> {
        let (files, has_quality_layers, has_version_tags) = self.detect_inputs();
        let draft = owox_core::detect_canon_draft(&owox_core::DetectSignals {
            files: &files,
            has_quality_layers,
            has_version_tags,
        });
        if draft.is_empty() {
            return self.envelope_result(Envelope::ok(
                "No guardrails inferred from existing code (no layered directories or destructive-infra signals). Author rules / quality by hand if needed.",
                serde_json::json!({ "layers": [], "boundaries": [], "irreversible": [] }),
            ));
        }
        self.envelope_result(Envelope::ok(
            "Reverse-generated draft guardrails from existing code. Proposal only — review with a human, then paste the snippets into quality.toml / rules.md (or add via canon.add). Nothing was written.",
            canon_draft_value(&draft),
        ))
    }

    /// セッション立ち上げを束ねる。向き付け・性質・既存コードからの逆生成案を1呼び出しで返す。
    #[tool(
        name = "kickoff",
        description = "Return kickoff orientation; reads only."
    )]
    async fn kickoff(&self) -> Result<CallToolResult, McpError> {
        let canon = owox_core::load_canon(&self.owox_dir).ok();
        let vision = canon
            .as_ref()
            .map(|c| c.brand.vision.clone())
            .filter(|v| !v.trim().is_empty());
        let phase = canon
            .as_ref()
            .map(|c| c.state.phase.as_str())
            .unwrap_or("initial");

        // 性質。宣言済みなら解決済み実効軸を、未宣言なら逆生成 draft を能動返却する (後入れ導線)。
        let declared = self.profile_declared();
        let (files, has_quality_layers, has_version_tags) = self.detect_inputs();
        let signals = owox_core::DetectSignals {
            files: &files,
            has_quality_layers,
            has_version_tags,
        };
        let nature = if declared {
            let axes = self.resolved_axes();
            serde_json::json!({
                "declared": true,
                "axes": {
                    "requirements-shape": axes.requirements_shape.as_str(),
                    "prioritization": axes.prioritization.as_str(),
                    "delivery": axes.delivery.as_str(),
                    "architecture": axes.architecture.as_str(),
                }
            })
        } else {
            serde_json::json!({
                "declared": false,
                "detected_draft": profile_draft_value(&owox_core::detect_profile(&signals)),
                "note": "Nature is not declared. Confirm this draft with a human, then set it with profile.set."
            })
        };

        // ガードレールが手薄な既存コードなら canon.detect 案を相乗りで返す (後入れの一筆書き)。
        let thin_guardrails = canon
            .as_ref()
            .map(|c| {
                c.quality.layers.is_empty()
                    && c.quality.boundaries.is_empty()
                    && c.rules.irreversible.is_empty()
            })
            .unwrap_or(true);
        let canon_draft = owox_core::detect_canon_draft(&signals);
        let guardrails = if thin_guardrails && !canon_draft.is_empty() {
            Some(canon_draft_value(&canon_draft))
        } else {
            None
        };

        let mut data = serde_json::json!({
            "vision": vision,
            "phase": phase,
            "nature": nature,
            "next": "Call the next tool for open decisions and ready tasks, and the context tool for what to read.",
        });
        if let Some(g) = guardrails {
            data["adopting_existing_code"] = serde_json::json!({
                "guardrails_draft": g,
                "note": "This codebase has thin guardrails. Review this draft with a human, then add via canon.add. Nothing was written.",
            });
        }
        self.envelope_result(Envelope::ok(
            "Oriented for this session. Stated below: Vision, phase, nature, and any reverse-generated drafts to confirm with a human. Nothing was written.",
            data,
        ))
    }

    /// 現在の性質 (解決済み実効軸) を返す。
    #[tool(
        name = "profile.get",
        description = "Get resolved nature axes and active methodology modules."
    )]
    async fn profile_get(&self) -> Result<CallToolResult, McpError> {
        let profile = owox_core::load_canon(&self.owox_dir)
            .map(|c| c.profile)
            .unwrap_or_default();
        let axes = match profile.resolve() {
            Ok(a) => a,
            Err(err) => return self.envelope_result(Envelope::failed(err)),
        };
        self.envelope_result(Envelope::ok(
            "Current project nature.",
            serde_json::json!({
                "preset": profile.preset,
                "axes": {
                    "requirements-shape": axes.requirements_shape.as_str(),
                    "prioritization": axes.prioritization.as_str(),
                    "delivery": axes.delivery.as_str(),
                    "architecture": axes.architecture.as_str(),
                },
                "active": {
                    "prfaq": axes.prfaq_active(),
                    "ideal-first": axes.ideal_first_active(),
                    "phased": axes.phased_active(),
                    "layered": axes.layered_active(),
                }
            }),
        ))
    }

    /// 現在のブランチの作業記憶へメモを足す。
    #[tool(
        name = "branch.note",
        description = "Append branch-scoped scratch work."
    )]
    async fn branch_note(
        &self,
        Parameters(p): Parameters<BranchNoteParams>,
    ) -> Result<CallToolResult, McpError> {
        let work_dir = self.repo_root();
        let branch = git_current_branch(work_dir);
        let work_root = branch_work_root(work_dir, &self.owox_dir);
        // work/ を git に乗せない (本体 repo の .owox/.gitignore へ)。
        if let Some(main_owox) = work_root.parent() {
            crate::cache::ensure_entry_ignored(main_owox, "work/");
        }
        self.envelope_result(owox_core::add_branch_note(
            &work_root,
            &branch,
            &today_utc(),
            &p.text,
        ))
    }

    /// 現在のブランチの作業記憶を読む (オンデマンド)。
    #[tool(
        name = "branch.notes",
        description = "Read the current branch's scratch work."
    )]
    async fn branch_notes(&self) -> Result<CallToolResult, McpError> {
        let work_dir = self.repo_root();
        let branch = git_current_branch(work_dir);
        let work_root = branch_work_root(work_dir, &self.owox_dir);
        self.envelope_result(owox_core::get_branch_memory_envelope(&work_root, &branch))
    }

    /// やることを 1 件作る。
    #[tool(
        name = "task.create",
        description = "Create a new task with links, dependencies, and optional external refs."
    )]
    async fn task_create(
        &self,
        Parameters(p): Parameters<TaskCreateParams>,
    ) -> Result<CallToolResult, McpError> {
        let deps = match parse_deps(p.deps) {
            Ok(d) => d,
            Err(err) => return self.envelope_result(Envelope::failed(err)),
        };
        let external = match parse_external(p.external) {
            Ok(e) => e,
            Err(err) => return self.envelope_result(Envelope::failed(err)),
        };
        let input = CreateTaskInput {
            title: p.title,
            links: p.links.into(),
            deps,
            layer: p.layer,
            stage: p.stage,
            external,
        };
        self.envelope_result(create_task(
            &self.owox_dir,
            &today_utc(),
            &self.known_layer_names(),
            input,
        ))
    }

    /// タスクを一覧する。ready=true で前提解決済のみ。
    #[tool(name = "task.list", description = "List tasks, or only ready tasks.")]
    async fn task_list(
        &self,
        Parameters(p): Parameters<TaskListParams>,
    ) -> Result<CallToolResult, McpError> {
        self.envelope_result(list_tasks_envelope(
            &self.owox_dir,
            p.ready,
            p.status.as_deref(),
        ))
    }

    /// タスクの title・状態・link・依存を変える (done は task.close)。
    #[tool(
        name = "task.update",
        description = "Update a task; title changes need a reason and done uses task.close."
    )]
    async fn task_update(
        &self,
        Parameters(p): Parameters<TaskUpdateParams>,
    ) -> Result<CallToolResult, McpError> {
        let add_deps = match parse_deps(p.deps) {
            Ok(d) => d,
            Err(err) => return self.envelope_result(Envelope::failed(err)),
        };
        let add_external = match parse_external(p.external) {
            Ok(e) => e,
            Err(err) => return self.envelope_result(Envelope::failed(err)),
        };
        self.envelope_result(update_task(
            &self.owox_dir,
            &today_utc(),
            &p.id,
            &self.known_layer_names(),
            UpdateTaskInput {
                title: p.title,
                status: p.status,
                links: p.links.map(Into::into),
                add_deps,
                reason: p.reason,
                layer: p.layer,
                stage: p.stage,
                add_external,
            },
        ))
    }

    /// タスクへ一時メモを追記する (来歴ではない軽量記録)。
    #[tool(name = "task.note", description = "Append a transient task note.")]
    async fn task_note(
        &self,
        Parameters(p): Parameters<TaskNoteParams>,
    ) -> Result<CallToolResult, McpError> {
        self.envelope_result(add_note(&self.owox_dir, &today_utc(), &p.id, &p.text))
    }

    /// タスクを依存でつなぐ。
    #[tool(name = "task.link", description = "Add a task dependency link.")]
    async fn task_link(
        &self,
        Parameters(p): Parameters<TaskLinkParams>,
    ) -> Result<CallToolResult, McpError> {
        let dep = match p.dep.into_dep() {
            Ok(d) => d,
            Err(err) => return self.envelope_result(Envelope::failed(err)),
        };
        self.envelope_result(link_task(&self.owox_dir, &p.id, dep))
    }

    /// タスクを閉じる。検証を通らないと閉じれない (自己申告 done を排除)。
    #[tool(
        name = "task.close",
        description = "Close a task as done only after configured verification passes."
    )]
    async fn task_close(
        &self,
        Parameters(p): Parameters<TaskCloseParams>,
    ) -> Result<CallToolResult, McpError> {
        let canon = match owox_core::load_canon(&self.owox_dir) {
            Ok(canon) => canon,
            Err(err) => {
                return self.envelope_result(Envelope::failed(format!("正本を読めない: {err}")));
            }
        };
        let work_dir = self.owox_dir.parent().unwrap_or(&self.owox_dir);
        self.envelope_result(close_task(
            &self.owox_dir,
            work_dir,
            &canon.verify.checks,
            &today_utc(),
            &p.id,
        ))
    }

    /// タスクを破棄する。理由を来歴へ残す (silent rot 禁止)。
    #[tool(
        name = "task.drop",
        description = "Drop an unneeded task with a recorded reason."
    )]
    async fn task_drop(
        &self,
        Parameters(p): Parameters<TaskDropParams>,
    ) -> Result<CallToolResult, McpError> {
        self.envelope_result(drop_task(&self.owox_dir, &today_utc(), &p.id, &p.reason))
    }

    /// スキルの 2 軸状態 (テスト・昇格) を一覧する。
    #[tool(
        name = "skill.list",
        description = "List project skills with test state and stage."
    )]
    async fn skill_list(&self) -> Result<CallToolResult, McpError> {
        self.envelope_result(list_skills_envelope(&self.owox_dir, self.repo_root()))
    }

    /// スキルのテストを実行し、合格・適格なら登録 (生成) する。
    #[tool(
        name = "skill.register",
        description = "Run skill lint and tests, then register on pass."
    )]
    async fn skill_register(
        &self,
        Parameters(p): Parameters<SkillIdParams>,
    ) -> Result<CallToolResult, McpError> {
        self.envelope_result(register_skill(&self.owox_dir, self.repo_root(), &p.id))
    }

    /// スキルを昇格する (人間ゲート)。人間承認後にだけ使う。
    #[tool(
        name = "skill.promote",
        description = "Promote a registered skill after human approval."
    )]
    async fn skill_promote(
        &self,
        Parameters(p): Parameters<SkillIdParams>,
    ) -> Result<CallToolResult, McpError> {
        self.envelope_result(promote_skill(
            &self.owox_dir,
            self.repo_root(),
            &today_utc(),
            &p.id,
        ))
    }

    /// スキルの経験メモリ (memory.md) へ追記する。
    #[tool(
        name = "skill.remember",
        description = "Append a lesson to a skill memory."
    )]
    async fn skill_remember(
        &self,
        Parameters(p): Parameters<SkillRememberParams>,
    ) -> Result<CallToolResult, McpError> {
        self.envelope_result(remember(&self.owox_dir, &today_utc(), &p.id, &p.text))
    }

    /// canon (brand / rules / practices / glossary) へ項目を追加する。追加は AI 直接 + 来歴。
    #[tool(
        name = "canon.add",
        description = "Add one canon item; changing or removing existing canon uses canon.propose."
    )]
    async fn canon_add(
        &self,
        Parameters(p): Parameters<CanonAddParams>,
    ) -> Result<CallToolResult, McpError> {
        self.envelope_result(owox_core::canon_add(
            &self.owox_dir,
            &today_utc(),
            &p.target,
            p.section.as_deref(),
            &p.text,
        ))
    }

    /// canon の変更・削除を提案する。canon は変えず人間判断点として返す。
    #[tool(
        name = "canon.propose",
        description = "Propose one canon change or removal for human decision."
    )]
    async fn canon_propose(
        &self,
        Parameters(p): Parameters<CanonProposeParams>,
    ) -> Result<CallToolResult, McpError> {
        self.envelope_result(owox_core::canon_propose(
            &self.owox_dir,
            &today_utc(),
            owox_core::ProposeInput {
                target: &p.target,
                op: p.op.as_deref(),
                section: p.section.as_deref(),
                item: p.item.as_deref(),
                to: p.to.as_deref(),
                change: p.change.as_deref(),
            },
        ))
    }

    /// 汎用経験 (skill + practices) を out_path へ持ち出す。持ち出しは人間ゲート。
    #[tool(
        name = "experience.export",
        description = "Export generic skills, scripts, and practices after secret scan and human review."
    )]
    async fn experience_export(
        &self,
        Parameters(p): Parameters<ExperienceExportParams>,
    ) -> Result<CallToolResult, McpError> {
        self.envelope_result(owox_core::experience_export(
            &self.owox_dir,
            std::path::Path::new(&p.out_path),
        ))
    }

    /// 別プロジェクトの経験束を取り込む。秘密検出時は人間ゲートで止める。
    #[tool(
        name = "experience.import",
        description = "Import generic skills as drafts and merge practices; refuse on secret scan."
    )]
    async fn experience_import(
        &self,
        Parameters(p): Parameters<ExperienceImportParams>,
    ) -> Result<CallToolResult, McpError> {
        self.envelope_result(owox_core::experience_import(
            &self.owox_dir,
            std::path::Path::new(&p.in_path),
        ))
    }

    /// 用語の定義を引く。canon を直読みせずここから取る。
    #[tool(
        name = "glossary.lookup",
        description = "Look up a project term definition."
    )]
    async fn glossary_lookup(
        &self,
        Parameters(p): Parameters<GlossaryTermParams>,
    ) -> Result<CallToolResult, McpError> {
        let env = glossary_lookup(&self.owox_dir, &p.term);
        let miss = env
            .data
            .as_ref()
            .and_then(|data| data.get("found"))
            .and_then(|found| found.as_bool())
            == Some(false);
        if miss {
            let current_session = crate::cache::current_session_id(&self.owox_dir);
            crate::cache::remember_glossary_hits(
                &self.owox_dir,
                current_session.as_deref(),
                &[owox_core::GlossaryTermHit {
                    term: p.term.clone(),
                    source: "lookup miss".to_string(),
                    example: p.term.clone(),
                }],
            );
        }
        self.envelope_result(env)
    }

    /// 運用指針を語で引く。床が肥大化で縮んだ後でも古い指針を取り出せる。
    #[tool(
        name = "practice.lookup",
        description = "Search operating practices by keyword."
    )]
    async fn practice_lookup(
        &self,
        Parameters(p): Parameters<PracticeLookupParams>,
    ) -> Result<CallToolResult, McpError> {
        self.envelope_result(owox_core::practice_lookup(&self.owox_dir, &p.query))
    }

    /// 調査知識を記録する。要約・出典を秘密走査し、supersedes 指定で旧を置き換える。
    #[tool(
        name = "knowledge.add",
        description = "Record research knowledge; scans for secrets."
    )]
    async fn knowledge_add(
        &self,
        Parameters(p): Parameters<KnowledgeAddParams>,
    ) -> Result<CallToolResult, McpError> {
        self.envelope_result(owox_core::add_knowledge(
            &self.owox_dir,
            &today_utc(),
            owox_core::KnowledgeInput {
                title: p.title,
                summary: p.summary,
                sources: p.sources,
                researched_on: p.researched_on,
                tags: p.tags,
                supersedes: p.supersedes,
            },
        ))
    }

    /// 調査知識を一覧する。状態・鮮度で絞れる。
    #[tool(
        name = "knowledge.list",
        description = "List knowledge entries with status and stale flag."
    )]
    async fn knowledge_list(
        &self,
        Parameters(p): Parameters<KnowledgeListParams>,
    ) -> Result<CallToolResult, McpError> {
        let stale_days = owox_core::load_canon(&self.owox_dir)
            .map(|c| c.quality.decay.knowledge_stale_days)
            .unwrap_or(90);
        self.envelope_result(owox_core::list_knowledge_envelope(
            &self.owox_dir,
            p.status.as_deref(),
            p.stale,
            &today_utc(),
            stale_days,
        ))
    }

    /// 調査知識を 1 件全文で読む。canon を直読みせずここから取る。
    #[tool(
        name = "knowledge.get",
        description = "Get one research knowledge entry."
    )]
    async fn knowledge_get(
        &self,
        Parameters(p): Parameters<KnowledgeGetParams>,
    ) -> Result<CallToolResult, McpError> {
        self.envelope_result(owox_core::get_knowledge(&self.owox_dir, &p.id))
    }

    /// 調査知識を語で引く。title / summary / tags に部分一致するものを返す。
    #[tool(
        name = "knowledge.lookup",
        description = "Search research knowledge summaries."
    )]
    async fn knowledge_lookup(
        &self,
        Parameters(p): Parameters<KnowledgeLookupParams>,
    ) -> Result<CallToolResult, McpError> {
        self.envelope_result(owox_core::lookup_knowledge(&self.owox_dir, &p.query))
    }

    /// プロジェクトの rules / policy をまとめて引く。canon を直読みせずここから取る。
    ///
    /// 床コンテキストは rules 本文を常時載せない (最小コンテキスト)。語トリガ push が外した時の
    /// backstop として AI が能動的に引ける (glossary.lookup と対称)。封筒でなく描画本文を返す。
    #[tool(
        name = "rules.lookup",
        description = "Get current common and active-phase rules."
    )]
    async fn rules_lookup(&self) -> Result<CallToolResult, McpError> {
        let canon = owox_core::load_canon(&self.owox_dir)
            .map_err(|err| McpError::internal_error(format!("正本を読めない: {err}"), None))?;
        Ok(self.text_result(owox_core::render_rules_block_for_phase(
            &canon.rules,
            canon.state.phase,
        )))
    }
}

/// 封筒の data (オブジェクト) へ 1 キーを足す。data が無ければ作る。
///
/// run_* の署名を変えずに advisory な追加情報 (routines 等) を載せるための後付け口。
fn merge_data(env: &mut Envelope, key: &str, value: serde_json::Value) {
    match env.data.as_mut().and_then(|d| d.as_object_mut()) {
        Some(obj) => {
            obj.insert(key.to_string(), value);
        }
        None => {
            env.data = Some(serde_json::json!({ key: value }));
        }
    }
}

fn attach_delivery_guidance(
    env: &mut Envelope,
    owox_dir: &Path,
    ops: &[owox_core::DeliveryOperation],
    paths: &[String],
) {
    let phase = owox_core::load_canon(owox_dir)
        .map(|canon| canon.state.phase)
        .unwrap_or(owox_core::Phase::Initial);
    let selection = owox_core::select_delivery_for_phase(
        owox_dir,
        owox_core::DeliveryRequest::for_operations(ops, paths),
        phase,
    )
    .unwrap_or_default();
    if selection.rules.is_empty() && selection.practices.is_empty() {
        return;
    }
    merge_data(
        env,
        "guidance",
        serde_json::json!({
            "rules": selection.rules,
            "practices": selection.practices,
        }),
    );
    let mut actions = env.next_actions.clone();
    if !selection.rules.is_empty() {
        actions.push("Review the relevant rules in data.guidance.rules.".to_string());
    }
    if !selection.practices.is_empty() {
        actions.push("Review the relevant practices in data.guidance.practices.".to_string());
    }
    env.next_actions = actions;
}

fn path_change_operations(paths: &[String]) -> Vec<owox_core::DeliveryOperation> {
    let mut out = Vec::new();
    for path in paths {
        if is_dependency_change_path(path) {
            push_delivery_operation(&mut out, owox_core::DeliveryOperation::DependencyChange);
        }
        if path.starts_with(".owox/requirements/") {
            push_delivery_operation(&mut out, owox_core::DeliveryOperation::RequirementChange);
        }
        if path.starts_with(".owox/skills/") {
            push_delivery_operation(&mut out, owox_core::DeliveryOperation::SkillChange);
        }
        if path.starts_with(".owox/") && !path.starts_with(".owox/skills/") {
            push_delivery_operation(&mut out, owox_core::DeliveryOperation::CanonChange);
        }
    }
    out
}

fn is_dependency_change_path(path: &str) -> bool {
    matches!(
        path,
        "Cargo.toml"
            | "Cargo.lock"
            | "package.json"
            | "package-lock.json"
            | "pnpm-lock.yaml"
            | "pyproject.toml"
            | "requirements.txt"
            | "go.mod"
            | "go.sum"
    )
}

fn push_delivery_operation(
    items: &mut Vec<owox_core::DeliveryOperation>,
    item: owox_core::DeliveryOperation,
) {
    if !items.contains(&item) {
        items.push(item);
    }
}

/// verify.run の封筒から検査の総合判定と失敗検査名を取り出す。commit ゲートが作業ツリー同一時に
/// 検査結果を再利用するための記録を組む。data が無い・形が想定外なら needs_human (検査未設定扱い)
/// に倒し、再利用させず commit ゲートに検査を走らせる (安全側)。
fn verify_outcome_from_envelope(env: &Envelope) -> (String, Vec<String>) {
    let Some(data) = env.data.as_ref() else {
        return ("needs_human".to_string(), Vec::new());
    };
    let verification = data
        .get("completion")
        .and_then(|c| c.get("verification"))
        .and_then(|v| v.as_str())
        .unwrap_or("needs_human")
        .to_string();
    let failed = data
        .get("results")
        .and_then(|r| r.as_array())
        .map(|arr| {
            arr.iter()
                .filter(|r| r.get("passed").and_then(serde_json::Value::as_bool) == Some(false))
                .filter_map(|r| {
                    r.get("check")
                        .and_then(serde_json::Value::as_str)
                        .map(str::to_string)
                })
                .collect()
        })
        .unwrap_or_default();
    (verification, failed)
}

/// 封筒へ現在任務を足し、JSON 文字列にしてテキストの tool 結果へ詰める。
///
/// structured_content でなくテキストにするのは Codex の対応が不確実なため
/// (`docs/decisions/20260613-Phase4-tool記録層.md`)。
fn envelope_result(
    mission: crate::cache::Mission,
    envelope: Envelope,
) -> Result<CallToolResult, McpError> {
    let mut value = serde_json::to_value(&envelope)
        .map_err(|e| McpError::internal_error(format!("封筒を値へ変換できない: {e}"), None))?;
    if let Some(obj) = value.as_object_mut() {
        obj.insert(
            "mission".to_string(),
            serde_json::Value::String(mission.as_str().to_string()),
        );
    }
    let json = serde_json::to_string_pretty(&value)
        .map_err(|e| McpError::internal_error(format!("封筒を直列化できない: {e}"), None))?;
    Ok(CallToolResult::success(vec![Content::text(json)]))
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for OwoxServer {
    /// 全 tool 呼び出しの単一集約点。使用履歴へ 1 行追記してから router へ委譲する。
    ///
    /// 入口コマンド (tool) の列を捉え、頻出手順の検知 (`routine.rs`) の素材にする。追記は name のみ
    /// (引数は残さない = 秘密漏れ・肥大の回避)・best-effort。`#[tool_handler]` は call_tool が無い時だけ
    /// 生成するので、ここで定義すれば自動生成を抑え 1 箇所に集約できる。
    async fn call_tool(
        &self,
        request: rmcp::model::CallToolRequestParams,
        context: rmcp::service::RequestContext<rmcp::RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        owox_core::usage::record(&self.owox_dir, &today_utc(), &request.name);
        let tcc = rmcp::handler::server::tool::ToolCallContext::new(self, request, context);
        self.tool_router.call(tcc).await
    }

    /// 能力告知。tool を持つことと、サーバ全体の使い方を伝える。
    ///
    /// 自前の get_info を持つので `#[tool_handler]` はこれを上書きしない。
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
        .with_instructions(
            "This server provides project navigation, a decision log, human gates, and verification. \
             To find what to read for the current task, call the context tool; for what to act on next, call the next tool. \
             Record durable design or direction decisions with decision.record (an open decision when something needs human judgment); keep transient working notes on a task with task.note. \
             Capture what the project must satisfy as requirements: requirement.create writes one, requirement.list and requirement.get read them (do not read .owox/ directly), requirement.update changes status or (with a reason) the title/statement, and requirement.add_criterion plus requirement.link_verification build the acceptance-criteria-to-test trace. \
             Manage work as tasks; to rename or re-scope a task use task.update with a reason, not a new task. \
             Declare the project phase with state.set (it adjusts how strict the gates are). \
             Grow reusable skills under .owox/skills/: skill.list shows their test and stage status, skill.register runs a contract lint and the skill's tests and generates it when they pass, skill.promote (human-approved only) elevates a registered skill and enables auto-invocation, and skill.remember appends a lesson to a skill's memory. A skill's tests check the behavior of the scripts and tools it depends on; a pure-prose skill with no testable scripts cannot opt into auto-invocation. When next or verify.run surfaces a frequently repeated step sequence under \"routines you could grow into a skill\", consider capturing it as a skill and routing it through register and promote. \
             Record research findings as knowledge: knowledge.add writes one (supersede-only updates), knowledge.lookup and knowledge.get read them on demand (do not read .owox/ directly), and knowledge.list shows their status and freshness; stale research surfaces in next and verify.run. \
             Edit the project canon uniformly: canon.add appends one item to brand, rules, practices, or glossary (AI-direct, recorded), while changing or removing an item is a human gate via canon.propose; look up a project term with glossary.lookup and the project's rules and policies with rules.lookup. When the human asks to change or remove a rule, value, principle, or definition, route it to canon.propose even if they do not name the tool. Grow operating practices from experience with canon.add target=practices. \
             When reviewing a change, call review.lenses to get the perspectives that apply to it, take in verify.run first, and review through each perspective — confirming and adversarially re-checking each finding; treat pruning as a proposal routed through the deletion policy and verification. \
             Run verify.run before finishing work. Do not read the canon under .owox/ directly; its guidance reaches you through the session context and these tools. You may author skills under .owox/skills/.",
        )
    }
}

/// `start` から上方へ `.owox` ディレクトリを探す。見つかればそのパス。
fn find_owox_dir(start: &Path) -> Option<PathBuf> {
    let mut dir = Some(start);
    while let Some(current) = dir {
        let candidate = current.join(".owox");
        if candidate.is_dir() {
            return Some(candidate);
        }
        dir = current.parent();
    }
    None
}

#[derive(Clone, Default)]
struct ReferenceSummary {
    requirement_refs: usize,
    decision_refs: usize,
    broken_refs: usize,
}

#[derive(Clone)]
struct ReferenceScan {
    summary: ReferenceSummary,
    broken: Vec<String>,
}

#[derive(Clone)]
struct GardeningFinding {
    kind: String,
    severity: &'static str,
    subject: String,
    detail: String,
}

#[derive(Clone)]
struct KickoffQuestion {
    stage: &'static str,
    item: String,
    recommendation: String,
    reason: String,
    decider: &'static str,
    options: Vec<String>,
}

struct DiffContextData {
    base: crate::files::DiffBase,
    changed_files: Vec<crate::files::ChangedFile>,
    canon_changes: Vec<crate::files::ChangedFile>,
    reference_summary: ReferenceSummary,
    review_hints: Vec<String>,
    gardening_hints: Vec<String>,
    needs_codebase: bool,
    guidance: owox_core::DeliverySelection,
    glossary_suggestions: Vec<owox_core::GlossarySuggestion>,
}

fn build_glossary_suggestions(
    repo_root: &Path,
    owox_dir: &Path,
    changed_paths: &[String],
    include_session: bool,
) -> Vec<owox_core::GlossarySuggestion> {
    let texts: Vec<_> = changed_paths
        .iter()
        .filter(|path| crate::files::classify_path(path) != crate::files::FileKind::Canon)
        .filter_map(|path| {
            std::fs::read_to_string(repo_root.join(path))
                .ok()
                .map(|text| owox_core::GlossaryScanText {
                    path: path.clone(),
                    text,
                })
        })
        .collect();
    let mut hits = owox_core::extract_term_hits(&texts, "changed file");
    if include_session {
        hits.extend(crate::cache::read_current_glossary_hits(owox_dir));
    }
    owox_core::suggest_terms_from_hits(owox_dir, &hits)
}

fn render_glossary_suggestions(suggestions: &[owox_core::GlossarySuggestion]) -> String {
    if suggestions.is_empty() {
        return String::new();
    }
    let mut out = String::from(
        "## Glossary candidates

",
    );
    out.push_str("These are possible project terms. Review them before adding definitions.\n\n");
    for suggestion in suggestions {
        out.push_str(&format!(
            "- {}: {}
",
            suggestion.term, suggestion.reason
        ));
        for example in &suggestion.examples {
            out.push_str(&format!("  - {example}\n"));
        }
    }
    out.push('\n');
    out
}

fn render_diff_context(
    repo_root: &Path,
    owox_dir: &Path,
    mission: crate::cache::Mission,
) -> Result<String, McpError> {
    let data = build_diff_context(repo_root, owox_dir)?;
    Ok(render_diff_context_body(&data, mission))
}

fn build_diff_context(repo_root: &Path, owox_dir: &Path) -> Result<DiffContextData, McpError> {
    let base = crate::files::main_merge_base(repo_root);
    let changed_files = crate::files::changed_files_since(repo_root, &base.rev);
    let canon_changes: Vec<_> = changed_files
        .iter()
        .filter(|f| f.kind.is_canon_surface())
        .cloned()
        .collect();
    let requirements = list_requirements(owox_dir)
        .map_err(|err| McpError::internal_error(format!("要件を読めない: {err}"), None))?;
    let decisions = list_decisions(owox_dir)
        .map_err(|err| McpError::internal_error(format!("来歴を読めない: {err}"), None))?;
    let reference_summary =
        scan_references(repo_root, &changed_files, &requirements, &decisions).summary;
    let needs_codebase = needs_codebase_map(&changed_files);
    let changed_paths: Vec<String> = changed_files.iter().map(|f| f.path.clone()).collect();
    let glossary_suggestions =
        build_glossary_suggestions(repo_root, owox_dir, &changed_paths, false);
    let canon = owox_core::load_canon(owox_dir).ok();
    let guidance = owox_core::select_delivery_for_phase(
        owox_dir,
        owox_core::DeliveryRequest::for_paths(&changed_paths),
        canon
            .as_ref()
            .map(|canon| canon.state.phase)
            .unwrap_or(owox_core::Phase::Initial),
    )
    .unwrap_or_default();
    let review_hints = diff_review_hints(
        &changed_files,
        &canon_changes,
        &reference_summary,
        needs_codebase,
    );
    let gardening_hints = canon
        .as_ref()
        .map(|canon| {
            let refs = scan_references(repo_root, &changed_files, &requirements, &decisions);
            build_gardening_findings(owox_dir, repo_root, &canon, &[], Some(&refs), false)
                .into_iter()
                .take(6)
                .map(|finding| {
                    format!("{} [{}]: {}", finding.subject, finding.kind, finding.detail)
                })
                .collect()
        })
        .unwrap_or_default();
    Ok(DiffContextData {
        base,
        changed_files,
        canon_changes,
        reference_summary,
        review_hints,
        gardening_hints,
        needs_codebase,
        guidance,
        glossary_suggestions,
    })
}

fn render_diff_context_body(data: &DiffContextData, mission: crate::cache::Mission) -> String {
    const MAX_FILES: usize = 40;
    let mut out = String::from("# Diff context\n\n");
    out.push_str(&format!(
        "Base: {} ({})\n\n",
        data.base.name,
        short_id(&data.base.rev)
    ));
    match mission {
        crate::cache::Mission::Kickoff => out.push_str(
            "Kickoff mission is active. Review canon and docs changes before implementation details.\n\n",
        ),
        crate::cache::Mission::Review => {
            out.push_str("Review mission is active. Read this map before deeper inspection.\n\n")
        }
        crate::cache::Mission::Verify => {
            out.push_str("Verify mission is active. Use this map to target checks and trace.\n\n")
        }
        crate::cache::Mission::Handoff => out.push_str(
            "Handoff mission is active. Surface the changed areas and remaining review pressure.\n\n",
        ),
        crate::cache::Mission::Work => {}
    }

    if !data.canon_changes.is_empty() {
        out.push_str("## Canon changes\n\n");
        render_changed_file_list(&mut out, &data.canon_changes, MAX_FILES);
    }

    out.push_str("## Changed files\n\n");
    if data.changed_files.is_empty() {
        out.push_str("No changed files detected.\n\n");
    } else {
        render_changed_file_list(&mut out, &data.changed_files, MAX_FILES);
    }

    out.push_str("## Reference summary\n\n");
    out.push_str(&format!(
        "- requirement refs: {}\n- decision refs: {}\n- broken refs: {}\n\n",
        data.reference_summary.requirement_refs,
        data.reference_summary.decision_refs,
        data.reference_summary.broken_refs
    ));

    out.push_str("## Review hints\n\n");
    for hint in &data.review_hints {
        out.push_str(&format!("- {hint}\n"));
    }
    out.push('\n');
    if !data.gardening_hints.is_empty() {
        out.push_str("## Gardening hints\n\n");
        for hint in &data.gardening_hints {
            out.push_str(&format!("- {hint}\n"));
        }
        out.push('\n');
    }

    if !data.glossary_suggestions.is_empty() {
        out.push_str(&render_glossary_suggestions(&data.glossary_suggestions));
    }

    let guidance = owox_core::render_delivery_block(&data.guidance);
    if !guidance.is_empty() {
        out.push_str(&guidance);
    }

    if data.needs_codebase {
        out.push_str("## Next\n\n");
        out.push_str("- If area ownership is unclear, call context with scope codebase.\n\n");
    }

    out
}

fn render_changed_file_list(
    out: &mut String,
    files: &[crate::files::ChangedFile],
    max_files: usize,
) {
    for file in files.iter().take(max_files) {
        out.push_str("- ");
        out.push_str(&file.path);
        out.push_str(" [");
        out.push_str(file.status.as_str());
        out.push_str(", ");
        out.push_str(file.kind.as_str());
        if let Some(from) = &file.previous_path {
            out.push_str(", from ");
            out.push_str(from);
        }
        out.push_str("]\n");
    }
    if files.len() > max_files {
        out.push_str(&format!("- ... and {} more\n", files.len() - max_files));
    }
    out.push('\n');
}

fn needs_codebase_map(changed_files: &[crate::files::ChangedFile]) -> bool {
    if changed_files
        .iter()
        .any(|f| f.kind == crate::files::FileKind::Unknown)
    {
        return true;
    }
    let areas: BTreeSet<String> = changed_files
        .iter()
        .map(|f| repo_area_key(&f.path))
        .collect();
    areas.len() >= 4
}

fn diff_review_hints(
    changed_files: &[crate::files::ChangedFile],
    canon_changes: &[crate::files::ChangedFile],
    reference_summary: &ReferenceSummary,
    needs_codebase: bool,
) -> Vec<String> {
    let mut hints = Vec::new();
    if !canon_changes.is_empty() {
        hints.push(
            "Canon or docs changed. Review that surface before implementation details.".to_string(),
        );
    }
    if changed_files
        .iter()
        .any(|f| f.kind == crate::files::FileKind::Generated)
    {
        hints.push(
            "Generated files changed. Confirm the generator path instead of direct edits."
                .to_string(),
        );
    }
    let changed_source = changed_files
        .iter()
        .any(|f| f.kind == crate::files::FileKind::Source);
    let changed_test = changed_files
        .iter()
        .any(|f| f.kind == crate::files::FileKind::Test);
    if changed_source && !changed_test {
        hints.push(
            "Source changed without test changes. Confirm whether coverage should move with it."
                .to_string(),
        );
    }
    if reference_summary.broken_refs > 0 {
        hints.push(format!(
            "{} broken owox reference(s) appear in the changed files.",
            reference_summary.broken_refs
        ));
    }
    if needs_codebase {
        hints.push("The change spans many areas or unknown paths. Use context with scope codebase before deeper edits.".to_string());
    }
    if hints.is_empty() {
        hints.push("No extra review pressure detected beyond the changed-file map.".to_string());
    }
    hints
}

#[derive(Debug, Clone)]
enum ReferenceTarget {
    Requirement { id: String, criterion: Option<u32> },
    Decision { id: String },
}

#[derive(Debug, Clone)]
struct ReferenceUse {
    path: String,
    kind: crate::files::FileKind,
}

struct ReferenceLookupData {
    query: String,
    target: ReferenceTarget,
    exists: bool,
    summary_lines: Vec<String>,
    used_by: Vec<ReferenceUse>,
    candidates: Vec<String>,
}

fn scan_references(
    repo_root: &Path,
    changed_files: &[crate::files::ChangedFile],
    requirements: &[owox_core::Requirement],
    decisions: &[owox_core::Decision],
) -> ReferenceScan {
    let requirement_map: BTreeMap<&str, BTreeSet<u32>> = requirements
        .iter()
        .map(|r| {
            (
                r.id.as_str(),
                r.criteria.iter().map(|c| c.id).collect::<BTreeSet<_>>(),
            )
        })
        .collect();
    let decision_ids: BTreeSet<&str> = decisions.iter().map(|d| d.id.as_str()).collect();
    let mut summary = ReferenceSummary::default();
    let mut broken = Vec::new();
    for file in changed_files {
        if file.status == crate::files::ChangeStatus::Deleted
            || file.kind == crate::files::FileKind::Generated
        {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(repo_root.join(&file.path)) else {
            continue;
        };
        for reference in extract_references(&text) {
            match reference {
                ReferenceTarget::Requirement { id, criterion } => {
                    summary.requirement_refs += 1;
                    match requirement_map.get(id.as_str()) {
                        Some(criteria) => {
                            if let Some(criterion) = criterion
                                && !criteria.contains(&criterion)
                            {
                                summary.broken_refs += 1;
                                broken.push(format!(
                                    "{} -> owox:req:{}#{}",
                                    file.path, id, criterion
                                ));
                            }
                        }
                        None => {
                            summary.broken_refs += 1;
                            broken.push(format!("{} -> owox:req:{}", file.path, id));
                        }
                    }
                }
                ReferenceTarget::Decision { id } => {
                    summary.decision_refs += 1;
                    if !decision_ids.contains(id.as_str()) {
                        summary.broken_refs += 1;
                        broken.push(format!("{} -> owox:dec:{}", file.path, id));
                    }
                }
            }
        }
    }
    ReferenceScan { summary, broken }
}

fn render_reference_context(
    repo_root: &Path,
    owox_dir: &Path,
    reference_id: &str,
    mission: crate::cache::Mission,
) -> Result<String, McpError> {
    let requirements = list_requirements(owox_dir)
        .map_err(|err| McpError::internal_error(format!("要件を読めない: {err}"), None))?;
    let decisions = list_decisions(owox_dir)
        .map_err(|err| McpError::internal_error(format!("来歴を読めない: {err}"), None))?;
    let data = build_reference_lookup_data(repo_root, reference_id, &requirements, &decisions)
        .ok_or_else(|| McpError::internal_error("参照IDを解釈できない".to_string(), None))?;
    Ok(render_reference_context_body(&data, mission))
}

fn build_reference_lookup_data(
    repo_root: &Path,
    reference_id: &str,
    requirements: &[owox_core::Requirement],
    decisions: &[owox_core::Decision],
) -> Option<ReferenceLookupData> {
    let target = parse_reference_query(reference_id)?;
    let mut exists = false;
    let mut summary_lines = Vec::new();
    let mut candidates = Vec::new();
    match &target {
        ReferenceTarget::Requirement { id, criterion } => {
            if let Some(requirement) = requirements
                .iter()
                .find(|requirement| requirement.id == *id)
            {
                exists = true;
                summary_lines.push(format!("requirement: {}", requirement.id));
                summary_lines.push(format!("title: {}", requirement.title));
                summary_lines.push(format!(
                    "status: {}",
                    requirement_status_label(requirement.status)
                ));
                if let Some(criterion_id) = criterion {
                    if let Some(found) = requirement
                        .criteria
                        .iter()
                        .find(|criterion| criterion.id == *criterion_id)
                    {
                        summary_lines.push(format!("criterion: #{}", found.id));
                        if !found.title.trim().is_empty() {
                            summary_lines.push(format!("criterion title: {}", found.title));
                        }
                        if !found.then.trim().is_empty() {
                            summary_lines.push(format!("criterion then: {}", found.then));
                        }
                    } else {
                        exists = false;
                        summary_lines.push(format!("missing criterion: #{}", criterion_id));
                        candidates.extend(requirement.criteria.iter().map(|criterion| {
                            format!("owox:req:{}#{}", requirement.id, criterion.id)
                        }));
                    }
                } else {
                    summary_lines.push(format!("criteria: {}", requirement.criteria.len()));
                }
            } else {
                candidates.extend(similar_requirement_refs(id, requirements));
            }
        }
        ReferenceTarget::Decision { id } => {
            if let Some(decision) = decisions.iter().find(|decision| decision.id == *id) {
                exists = true;
                summary_lines.push(format!("decision: {}", decision.id));
                summary_lines.push(format!("title: {}", decision.title));
                summary_lines.push(format!(
                    "status: {}",
                    decision_status_label(decision.status)
                ));
            } else {
                candidates.extend(similar_decision_refs(id, decisions));
            }
        }
    }
    let used_by = collect_reference_uses(repo_root, &target);
    Some(ReferenceLookupData {
        query: reference_id.trim().to_string(),
        target,
        exists,
        summary_lines,
        used_by,
        candidates,
    })
}

fn render_reference_context_body(
    data: &ReferenceLookupData,
    mission: crate::cache::Mission,
) -> String {
    let mut out = String::from("# Reference context\n\n");
    match mission {
        crate::cache::Mission::Kickoff => out.push_str(
            "Kickoff mission is active. Use this to confirm trace before writing canon.\n\n",
        ),
        crate::cache::Mission::Review => {
            out.push_str("Review mission is active. Use this to trace why the change exists.\n\n")
        }
        crate::cache::Mission::Verify => {
            out.push_str("Verify mission is active. Use this to confirm trace coverage.\n\n")
        }
        crate::cache::Mission::Handoff => out.push_str(
            "Handoff mission is active. Use this to point the next session at the right source.\n\n",
        ),
        crate::cache::Mission::Work => {}
    }
    out.push_str(&format!("Reference: {}\n\n", data.query));
    out.push_str("## Status\n\n");
    if data.exists {
        out.push_str("- target exists\n");
    } else {
        out.push_str("- target missing\n");
    }
    out.push_str(&format!("- usages: {}\n\n", data.used_by.len()));

    out.push_str("## Target\n\n");
    if data.summary_lines.is_empty() {
        out.push_str("- no matching requirement or decision found\n\n");
    } else {
        for line in &data.summary_lines {
            out.push_str(&format!("- {line}\n"));
        }
        out.push('\n');
    }

    out.push_str("## Used by\n\n");
    if data.used_by.is_empty() {
        out.push_str("- no references found in repo text files\n\n");
    } else {
        for item in &data.used_by {
            out.push_str(&format!("- {} [{}]\n", item.path, item.kind.as_str()));
        }
        out.push('\n');
    }

    if !data.candidates.is_empty() {
        out.push_str("## Candidates\n\n");
        for candidate in data.candidates.iter().take(8) {
            out.push_str(&format!("- {candidate}\n"));
        }
        out.push('\n');
    }

    out.push_str("## Next\n\n");
    match &data.target {
        ReferenceTarget::Requirement { id, .. } if data.exists => {
            out.push_str(&format!(
                "- Call requirement.get with id {} for the full requirement.\n",
                id
            ));
        }
        ReferenceTarget::Decision { .. } if data.exists => {
            out.push_str("- Use this summary as the decision read path, then run verify.run if trace is still unclear.\n");
        }
        _ => {
            out.push_str("- Fix the reference or choose one of the candidates above.\n");
        }
    }
    out.push_str("- Run verify.run to see broken owox references in the current change.\n\n");
    out
}

fn extract_references(text: &str) -> Vec<ReferenceTarget> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(index) = rest.find("owox:") {
        rest = &rest[index + 5..];
        if let Some(next) = rest.strip_prefix("req:") {
            let token = take_reference_token(next);
            if !token.is_empty() {
                let (id, criterion) = match token.split_once('#') {
                    Some((id, number)) => (id.to_string(), number.parse::<u32>().ok()),
                    None => (token.to_string(), None),
                };
                out.push(ReferenceTarget::Requirement { id, criterion });
            }
            rest = next;
            continue;
        }
        if let Some(next) = rest.strip_prefix("dec:") {
            let token = take_reference_token(next);
            if !token.is_empty() {
                out.push(ReferenceTarget::Decision {
                    id: token.to_string(),
                });
            }
            rest = next;
            continue;
        }
    }
    out
}

fn parse_reference_query(text: &str) -> Option<ReferenceTarget> {
    let trimmed = text.trim();
    if let Some(next) = trimmed.strip_prefix("owox:req:") {
        let token = take_reference_token(next);
        if token.is_empty() {
            return None;
        }
        let (id, criterion) = match token.split_once('#') {
            Some((id, number)) => (id.to_string(), number.parse::<u32>().ok()),
            None => (token.to_string(), None),
        };
        return Some(ReferenceTarget::Requirement { id, criterion });
    }
    if let Some(next) = trimmed.strip_prefix("owox:dec:") {
        let token = take_reference_token(next);
        if token.is_empty() {
            return None;
        }
        return Some(ReferenceTarget::Decision {
            id: token.to_string(),
        });
    }
    None
}

fn take_reference_token(text: &str) -> &str {
    let end = text
        .char_indices()
        .find(|(_, ch)| {
            ch.is_whitespace()
                || matches!(
                    ch,
                    ')' | '(' | '[' | ']' | '{' | '}' | '<' | '>' | '"' | '\'' | ',' | ';'
                )
        })
        .map(|(i, _)| i)
        .unwrap_or(text.len());
    text[..end].trim_end_matches(['.', ':', '!', '?'])
}

fn collect_reference_uses(repo_root: &Path, target: &ReferenceTarget) -> Vec<ReferenceUse> {
    let mut paths = crate::files::list_repo_files(repo_root);
    paths.extend(reference_canon_files(repo_root));
    paths.sort();
    paths.dedup();
    let mut uses = Vec::new();
    for path in paths {
        let kind = crate::files::classify_path(&path);
        if kind == crate::files::FileKind::Generated {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(repo_root.join(&path)) else {
            continue;
        };
        if extract_references(&text)
            .iter()
            .any(|found| reference_matches(target, found))
        {
            uses.push(ReferenceUse { path, kind });
        }
    }
    uses
}

fn reference_canon_files(repo_root: &Path) -> Vec<String> {
    let mut out = Vec::new();
    for rel in [".owox/requirements", ".owox/decisions"] {
        let dir = repo_root.join(rel);
        if dir.exists() {
            walk_reference_files(repo_root, &dir, &mut out);
        }
    }
    out
}

fn walk_reference_files(root: &Path, dir: &Path, out: &mut Vec<String>) {
    let Ok(read_dir) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in read_dir.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk_reference_files(root, &path, out);
            continue;
        }
        let Ok(rel) = path.strip_prefix(root) else {
            continue;
        };
        out.push(rel.to_string_lossy().replace('\\', "/"));
    }
}

fn reference_matches(target: &ReferenceTarget, found: &ReferenceTarget) -> bool {
    match (target, found) {
        (
            ReferenceTarget::Requirement {
                id: left_id,
                criterion: None,
            },
            ReferenceTarget::Requirement { id: right_id, .. },
        ) => left_id == right_id,
        (
            ReferenceTarget::Requirement {
                id: left_id,
                criterion: Some(left_criterion),
            },
            ReferenceTarget::Requirement {
                id: right_id,
                criterion: Some(right_criterion),
            },
        ) => left_id == right_id && left_criterion == right_criterion,
        (ReferenceTarget::Decision { id: left_id }, ReferenceTarget::Decision { id: right_id }) => {
            left_id == right_id
        }
        _ => false,
    }
}

fn similar_requirement_refs(id: &str, requirements: &[owox_core::Requirement]) -> Vec<String> {
    similar_ids(
        id,
        requirements
            .iter()
            .map(|requirement| requirement.id.as_str()),
    )
    .into_iter()
    .map(|candidate| format!("owox:req:{candidate}"))
    .collect()
}

fn similar_decision_refs(id: &str, decisions: &[owox_core::Decision]) -> Vec<String> {
    similar_ids(id, decisions.iter().map(|decision| decision.id.as_str()))
        .into_iter()
        .map(|candidate| format!("owox:dec:{candidate}"))
        .collect()
}

fn similar_ids<'a>(query: &str, ids: impl Iterator<Item = &'a str>) -> Vec<String> {
    let lower = query.to_lowercase();
    ids.filter(|candidate| {
        let candidate_lower = candidate.to_lowercase();
        candidate_lower.contains(&lower) || lower.contains(&candidate_lower)
    })
    .take(8)
    .map(str::to_string)
    .collect()
}

fn requirement_status_label(status: owox_core::RequirementStatus) -> &'static str {
    match status {
        owox_core::RequirementStatus::Draft => "draft",
        owox_core::RequirementStatus::Accepted => "accepted",
        owox_core::RequirementStatus::Superseded => "superseded",
    }
}

fn decision_status_label(status: DecisionStatus) -> &'static str {
    match status {
        DecisionStatus::Open => "open",
        DecisionStatus::Adopted => "adopted",
        DecisionStatus::Rejected => "rejected",
        DecisionStatus::Superseded => "superseded",
    }
}

fn short_id(value: &str) -> &str {
    value.get(..8).unwrap_or(value)
}

const CODEBASE_INDEX_STALE_DAYS: i64 = 7;
const GARDENING_FLOOR_BLOAT_TOKENS: usize = 3000;
const GARDENING_LOW_USE_DAYS: i64 = 30;
const GARDENING_LOW_USE_COMMITS: usize = 20;

#[derive(Clone, Default)]
struct CodebaseCacheStatus {
    stale: bool,
    refreshed: bool,
    reasons: Vec<String>,
}

fn repo_area_key(path: &str) -> String {
    let parts: Vec<&str> = path.split('/').collect();
    match parts.as_slice() {
        ["crates", crate_name, "src", ..] => format!("crates/{crate_name}/src"),
        ["crates", crate_name, "tests", ..] => format!("crates/{crate_name}/tests"),
        ["docs", ..] => "docs".to_string(),
        [".owox", ..] => ".owox".to_string(),
        [".codex", ..] => ".codex".to_string(),
        ["src", ..] => "src".to_string(),
        ["tests", ..] => "tests".to_string(),
        ["scripts", ..] => "scripts".to_string(),
        [first, ..] => (*first).to_string(),
        [] => ".".to_string(),
    }
}

fn render_codebase_context(
    repo_root: &Path,
    owox_dir: &Path,
    mission: crate::cache::Mission,
) -> Result<String, McpError> {
    let head = crate::files::current_git_head(repo_root);
    let today = today_utc();
    let (index, cache) = match crate::cache::read_codebase_index(owox_dir) {
        Some(index) => {
            let reasons = codebase_stale_reasons(repo_root, &index, head.as_deref(), &today);
            if reasons.is_empty() {
                (
                    index,
                    CodebaseCacheStatus {
                        stale: false,
                        refreshed: false,
                        reasons: Vec::new(),
                    },
                )
            } else {
                let index = build_codebase_index(repo_root, head.clone());
                let _ = crate::cache::write_codebase_index(owox_dir, &index);
                (
                    index,
                    CodebaseCacheStatus {
                        stale: false,
                        refreshed: true,
                        reasons,
                    },
                )
            }
        }
        None => {
            let index = build_codebase_index(repo_root, head.clone());
            let _ = crate::cache::write_codebase_index(owox_dir, &index);
            (
                index,
                CodebaseCacheStatus {
                    stale: false,
                    refreshed: true,
                    reasons: vec!["cache missing".to_string()],
                },
            )
        }
    };
    let related = current_diff_paths(repo_root);
    Ok(render_codebase_context_body(
        &index, &related, mission, &cache,
    ))
}

fn build_codebase_index(repo_root: &Path, git_head: Option<String>) -> crate::cache::CodebaseIndex {
    let files = crate::files::list_repo_files(repo_root);
    let package_files = detect_package_files(&files);
    let areas = detect_codebase_areas(&files);
    let entrypoints = detect_entrypoints(&files);
    let checks = detect_checks(&package_files);
    let generated_or_external = detect_generated_dirs(repo_root);
    let source_files = source_files_for_index(&package_files, &areas, &entrypoints);
    crate::cache::CodebaseIndex {
        root_kind: detect_root_kind(repo_root),
        package_files,
        areas,
        entrypoints,
        checks,
        generated_or_external,
        source_files,
        git_head,
        generated_on: today_utc(),
    }
}

fn detect_root_kind(repo_root: &Path) -> String {
    let cargo = repo_root.join("Cargo.toml");
    if let Ok(text) = std::fs::read_to_string(&cargo) {
        if text.contains("[workspace]") {
            return "rust-workspace".to_string();
        }
        return "rust-crate".to_string();
    }
    if repo_root.join("package.json").exists() {
        return "node-project".to_string();
    }
    "git-repo".to_string()
}

fn detect_package_files(files: &[String]) -> Vec<String> {
    files
        .iter()
        .filter(|f| !f.contains('/'))
        .filter(|f| {
            matches!(
                f.as_str(),
                "Cargo.toml" | "package.json" | "pyproject.toml" | "go.mod"
            )
        })
        .cloned()
        .collect()
}

fn detect_codebase_areas(files: &[String]) -> Vec<crate::cache::CodebaseArea> {
    let mut grouped: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for file in files {
        let key = repo_area_key(file);
        if key == "." {
            continue;
        }
        grouped.entry(key).or_default().push(file.clone());
    }
    grouped
        .into_iter()
        .filter_map(|(path, files)| {
            let evidence: Vec<String> = files.into_iter().take(2).collect();
            let kind = infer_area_kind(&path, &evidence);
            if kind == crate::files::FileKind::Config || kind == crate::files::FileKind::Unknown {
                return None;
            }
            Some(crate::cache::CodebaseArea {
                role: infer_area_role(&path, kind, &evidence),
                kind: kind.as_str().to_string(),
                path,
                evidence,
            })
        })
        .collect()
}

fn infer_area_kind(path: &str, evidence: &[String]) -> crate::files::FileKind {
    if path == ".owox" || path == ".codex" {
        return crate::files::FileKind::Canon;
    }
    if path == "docs" {
        return crate::files::FileKind::Docs;
    }
    if path == "tests" || path.ends_with("/tests") {
        return crate::files::FileKind::Test;
    }
    let mut counts: BTreeMap<&'static str, usize> = BTreeMap::new();
    for file in evidence {
        *counts
            .entry(crate::files::classify_path(file).as_str())
            .or_default() += 1;
    }
    if counts.contains_key("source") {
        crate::files::FileKind::Source
    } else if counts.contains_key("test") {
        crate::files::FileKind::Test
    } else if counts.contains_key("docs") {
        crate::files::FileKind::Docs
    } else if counts.contains_key("canon") {
        crate::files::FileKind::Canon
    } else if counts.contains_key("generated") {
        crate::files::FileKind::Generated
    } else {
        crate::files::FileKind::Unknown
    }
}

fn infer_area_role(path: &str, kind: crate::files::FileKind, evidence: &[String]) -> String {
    match (path, kind) {
        (".owox", _) => "Project canon".to_string(),
        (".codex", _) => "Control harness".to_string(),
        ("docs", _) => "Project docs".to_string(),
        ("scripts", _) => "Automation scripts".to_string(),
        (_, crate::files::FileKind::Test) => "Tests".to_string(),
        (_, crate::files::FileKind::Generated) => "Generated or external output".to_string(),
        (_, crate::files::FileKind::Source)
            if evidence
                .iter()
                .any(|f| f.ends_with("/main.rs") || f == "src/main.rs") =>
        {
            "Executable surface".to_string()
        }
        (_, crate::files::FileKind::Source)
            if evidence
                .iter()
                .any(|f| f.ends_with("/lib.rs") || f == "src/lib.rs") =>
        {
            "Library source".to_string()
        }
        (_, crate::files::FileKind::Source) => "Source code".to_string(),
        (_, crate::files::FileKind::Docs) => "Documentation".to_string(),
        _ => "Repo area".to_string(),
    }
}

fn detect_entrypoints(files: &[String]) -> Vec<String> {
    files
        .iter()
        .filter(|f| {
            f.ends_with("/src/main.rs")
                || *f == "src/main.rs"
                || f.ends_with("/src/lib.rs")
                || *f == "src/lib.rs"
                || f.starts_with("bin/")
        })
        .cloned()
        .collect()
}

fn detect_checks(package_files: &[String]) -> Vec<String> {
    if package_files.iter().any(|f| f == "Cargo.toml") {
        return vec!["cargo test".to_string(), "cargo clippy".to_string()];
    }
    if package_files.iter().any(|f| f == "package.json") {
        return vec!["npm test".to_string()];
    }
    Vec::new()
}

fn detect_generated_dirs(repo_root: &Path) -> Vec<String> {
    let mut out = Vec::new();
    for path in [
        "target",
        ".agents",
        "dist",
        "build",
        "coverage",
        "node_modules",
        "vendor",
    ] {
        if repo_root.join(path).exists() {
            out.push(format!("{path}/"));
        }
    }
    out
}

fn source_files_for_index(
    package_files: &[String],
    areas: &[crate::cache::CodebaseArea],
    entrypoints: &[String],
) -> Vec<String> {
    let mut set = BTreeSet::new();
    set.extend(package_files.iter().cloned());
    set.extend(entrypoints.iter().cloned());
    for area in areas {
        set.extend(area.evidence.iter().cloned());
    }
    set.into_iter().collect()
}

fn current_diff_paths(repo_root: &Path) -> Vec<String> {
    let base = crate::files::main_merge_base(repo_root);
    crate::files::changed_files_since(repo_root, &base.rev)
        .into_iter()
        .map(|f| f.path)
        .take(8)
        .collect()
}

fn detect_inputs(repo_root: &Path, owox_dir: &Path) -> (Vec<String>, bool, bool) {
    let files = crate::files::list_repo_files(repo_root);
    let has_quality_layers = owox_core::load_canon(owox_dir)
        .map(|c| !c.quality.layers.is_empty() || !c.quality.boundaries.is_empty())
        .unwrap_or(false);
    let has_version_tags = git_has_version_tags(repo_root);
    (files, has_quality_layers, has_version_tags)
}

fn codebase_stale_reasons(
    repo_root: &Path,
    index: &crate::cache::CodebaseIndex,
    current_head: Option<&str>,
    today: &str,
) -> Vec<String> {
    let mut reasons = Vec::new();
    if index.git_head.as_deref() != current_head {
        reasons.push("git head changed".to_string());
    }
    for path in &index.source_files {
        if !repo_root.join(path).exists() {
            reasons.push(format!("evidence file missing: {path}"));
        }
    }
    let changed: BTreeSet<String> = crate::files::changed_files(repo_root).into_iter().collect();
    for path in &index.source_files {
        if changed.contains(path) {
            reasons.push(format!("evidence file changed: {path}"));
        }
    }
    if let (Some(today_days), Some(generated_days)) =
        (ymd_to_days(today), ymd_to_days(&index.generated_on))
    {
        let age = today_days - generated_days;
        if age > CODEBASE_INDEX_STALE_DAYS {
            reasons.push(format!(
                "cache age {age} days exceeds {CODEBASE_INDEX_STALE_DAYS} days"
            ));
        }
    }
    reasons.sort();
    reasons.dedup();
    reasons
}

fn ymd_to_days(s: &str) -> Option<i64> {
    if s.len() != 8 || !s.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let y = s[0..4].parse::<i64>().ok()?;
    let m = s[4..6].parse::<u32>().ok()?;
    let d = s[6..8].parse::<u32>().ok()?;
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    Some(days_from_civil(y, m, d))
}

fn build_gardening_findings(
    owox_dir: &Path,
    repo_root: &Path,
    canon: &owox_core::Canon,
    decay: &[DecayFinding],
    refs: Option<&ReferenceScan>,
    include_skills: bool,
) -> Vec<GardeningFinding> {
    let mut findings = Vec::new();
    findings.extend(glossary_duplicate_findings(&canon.glossary));
    findings.extend(decay_gardening_findings(decay));
    findings.extend(low_use_findings(owox_dir, repo_root, canon, &today_utc()));
    findings.extend(floor_bloat_findings(canon));
    findings.extend(generated_edit_findings(repo_root));
    findings.extend(generated_drift_findings(owox_dir, repo_root, canon));
    findings.extend(command_routing_findings(owox_dir));
    if include_skills {
        findings.extend(skill_gardening_findings(owox_dir, repo_root));
    }
    if let Some(refs) = refs {
        findings.extend(reference_gardening_findings(refs));
        findings.extend(untraced_canon_change_findings(repo_root, refs));
    }
    findings.sort_by(|a, b| {
        (b.severity == "failed")
            .cmp(&(a.severity == "failed"))
            .then(a.kind.cmp(&b.kind))
            .then(a.subject.cmp(&b.subject))
    });
    findings
}

fn glossary_duplicate_findings(glossary: &owox_core::Glossary) -> Vec<GardeningFinding> {
    let mut seen: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for entry in &glossary.entries {
        seen.entry(entry.term.to_lowercase())
            .or_default()
            .push(entry.term.clone());
        for alias in &entry.aliases {
            seen.entry(alias.to_lowercase())
                .or_default()
                .push(format!("{} (alias)", entry.term));
        }
    }
    seen.into_iter()
        .filter(|(_, owners)| owners.len() > 1)
        .map(|(name, owners)| GardeningFinding {
            kind: "duplicate-glossary".to_string(),
            severity: "advisory",
            subject: name,
            detail: format!("reused by {}", owners.join(", ")),
        })
        .collect()
}

fn decay_gardening_findings(decay: &[DecayFinding]) -> Vec<GardeningFinding> {
    decay
        .iter()
        .filter_map(|finding| {
            let kind = match finding.kind {
                "redundant-practice" => "duplicate-practice",
                "stale-practice"
                | "stale-knowledge"
                | "stale-branch-memory"
                | "stale-open-decision"
                | "review-decision" => finding.kind,
                _ => return None,
            };
            Some(GardeningFinding {
                kind: kind.to_string(),
                severity: "advisory",
                subject: finding.subject.clone(),
                detail: finding.detail.clone(),
            })
        })
        .collect()
}

fn low_use_findings(
    owox_dir: &Path,
    repo_root: &Path,
    canon: &owox_core::Canon,
    today: &str,
) -> Vec<GardeningFinding> {
    let mut findings = low_use_skill_findings(owox_dir, repo_root, today);
    findings.extend(low_use_practice_findings(repo_root, canon, today));
    findings
}

fn low_use_skill_findings(owox_dir: &Path, repo_root: &Path, today: &str) -> Vec<GardeningFinding> {
    let skills = owox_core::load_skills(owox_dir).unwrap_or_default();
    if skills.is_empty() {
        return Vec::new();
    }
    let corpus = gardening_search_corpus(owox_dir);
    skills
        .into_iter()
        .filter_map(|skill| {
            if skill_is_referenced(&corpus, &skill) {
                return None;
            }
            let rel = Path::new(".owox").join("skills").join(&skill.id);
            let (updated_on, commits_since) = git_path_activity(repo_root, &rel)?;
            let age = age_days(today, &updated_on)?;
            if age <= GARDENING_LOW_USE_DAYS || commits_since < GARDENING_LOW_USE_COMMITS {
                return None;
            }
            Some(GardeningFinding {
                kind: "low-use".to_string(),
                severity: "advisory",
                subject: format!("skill {}", skill.id),
                detail: format!(
                    "unreferenced in canon or entry text for {age} days, with {commits_since} repo commits since its last update"
                ),
            })
        })
        .collect()
}

fn low_use_practice_findings(
    repo_root: &Path,
    canon: &owox_core::Canon,
    today: &str,
) -> Vec<GardeningFinding> {
    let floor_max = canon.settings.context.practices_floor_max;
    if canon.practices.entries.len() <= floor_max {
        return Vec::new();
    }
    let mut practices = canon.practices.entries.clone();
    practices.sort_by(|a, b| b.date.cmp(&a.date));
    practices
        .into_iter()
        .skip(floor_max)
        .filter_map(|practice| {
            let age = age_days(today, &practice.date)?;
            let commits_since = git_commit_count_since_date(repo_root, &practice.date)?;
            if age <= GARDENING_LOW_USE_DAYS || commits_since < GARDENING_LOW_USE_COMMITS {
                return None;
            }
            Some(GardeningFinding {
                kind: "low-use".to_string(),
                severity: "advisory",
                subject: format!("practice {}", practice.date),
                detail: format!(
                    "outside the freshest {floor_max} practices for {age} days, with {commits_since} repo commits since it was added"
                ),
            })
        })
        .collect()
}

fn floor_bloat_findings(canon: &owox_core::Canon) -> Vec<GardeningFinding> {
    let floor = owox_core::floor_context(canon);
    let tokens = owox_core::tokens::estimate_tokens(&floor);
    if tokens <= GARDENING_FLOOR_BLOAT_TOKENS {
        return Vec::new();
    }
    vec![GardeningFinding {
        kind: "floor-bloat".to_string(),
        severity: "advisory",
        subject: "SessionStart".to_string(),
        detail: format!(
            "floor context is about {tokens} tokens; threshold is {GARDENING_FLOOR_BLOAT_TOKENS}"
        ),
    }]
}

fn generated_edit_findings(repo_root: &Path) -> Vec<GardeningFinding> {
    crate::files::changed_files_since(repo_root, &crate::files::main_merge_base(repo_root).rev)
        .into_iter()
        .filter(|file| file.kind == crate::files::FileKind::Generated)
        .map(|file| GardeningFinding {
            kind: "generated-edit".to_string(),
            severity: "failed",
            subject: file.path,
            detail: "generated or external output changed directly".to_string(),
        })
        .collect()
}

fn generated_drift_findings(
    owox_dir: &Path,
    repo_root: &Path,
    canon: &owox_core::Canon,
) -> Vec<GardeningFinding> {
    let mut findings = Vec::new();
    for (dest, file) in expected_generated_files(owox_dir, repo_root, canon) {
        if !dest.exists() {
            continue;
        }
        let Ok(expected) = owox_core::render_generated_file(&dest, &file) else {
            continue;
        };
        let actual = std::fs::read_to_string(&dest).unwrap_or_default();
        if actual != expected {
            findings.push(GardeningFinding {
                kind: "generated-drift".to_string(),
                severity: "failed",
                subject: file.path,
                detail: "generated file differs from what the current canon would render"
                    .to_string(),
            });
            continue;
        }
        if file.executable && !file_is_executable(&dest) {
            findings.push(GardeningFinding {
                kind: "generated-drift".to_string(),
                severity: "failed",
                subject: file.path,
                detail: "generated file is expected to be executable".to_string(),
            });
        }
    }
    findings
}

fn skill_gardening_findings(owox_dir: &Path, repo_root: &Path) -> Vec<GardeningFinding> {
    let skills = owox_core::load_skills(owox_dir).unwrap_or_default();
    let mut findings = Vec::new();
    for skill in skills {
        let status = owox_core::skill_status(&skill, repo_root);
        if let Some(problem) = status.problem {
            findings.push(GardeningFinding {
                kind: "broken-skill".to_string(),
                severity: "failed",
                subject: status.id,
                detail: problem,
            });
        } else if status.tests == owox_core::TestState::Failing {
            findings.push(GardeningFinding {
                kind: "broken-skill".to_string(),
                severity: "failed",
                subject: status.id,
                detail: "skill tests are failing".to_string(),
            });
        }
    }
    findings
}

fn reference_gardening_findings(refs: &ReferenceScan) -> Vec<GardeningFinding> {
    refs.broken
        .iter()
        .map(|broken| GardeningFinding {
            kind: "broken-reference".to_string(),
            severity: "failed",
            subject: broken.clone(),
            detail: "broken owox reference".to_string(),
        })
        .collect()
}

fn command_routing_findings(owox_dir: &Path) -> Vec<GardeningFinding> {
    let commands = match owox_core::load_commands(owox_dir) {
        Ok(commands) => commands,
        Err(_) => return Vec::new(),
    };
    let expected: &[(&str, &[&str])] = &[
        ("kickoff", &["mission.start", "next"]),
        ("next", &["next", "context"]),
        ("status", &["next", "gate.list"]),
        ("verify", &["verify.run"]),
        ("review", &["review.lenses", "verify.run", "context"]),
        ("skill", &["skill.list"]),
    ];
    let mut findings = Vec::new();
    for (name, required) in expected {
        let Some(command) = commands.iter().find(|command| command.name == *name) else {
            findings.push(GardeningFinding {
                kind: "entry-routing".to_string(),
                severity: "advisory",
                subject: (*name).to_string(),
                detail: "entry command is missing".to_string(),
            });
            continue;
        };
        let missing: Vec<&str> = required
            .iter()
            .copied()
            .filter(|needle| !command.body.contains(needle))
            .collect();
        if !missing.is_empty() {
            findings.push(GardeningFinding {
                kind: "entry-routing".to_string(),
                severity: "advisory",
                subject: (*name).to_string(),
                detail: format!("body no longer points to {}", missing.join(", ")),
            });
        }
    }
    findings
}

fn gardening_search_corpus(owox_dir: &Path) -> String {
    ["brand.md", "rules.md", "context.md", "commands.toml"]
        .into_iter()
        .filter_map(|name| std::fs::read_to_string(owox_dir.join(name)).ok())
        .collect::<Vec<_>>()
        .join("\n")
        .to_lowercase()
}

fn skill_is_referenced(corpus: &str, skill: &owox_core::Skill) -> bool {
    let mut needles = vec![skill.id.to_lowercase()];
    let name = skill.name.trim().to_lowercase();
    if !name.is_empty() && name != needles[0] {
        needles.push(name);
    }
    needles
        .into_iter()
        .filter(|needle| !needle.is_empty())
        .any(|needle| corpus.contains(&needle))
}

fn git_path_activity(repo_root: &Path, rel: &Path) -> Option<(String, usize)> {
    let rel = rel.to_string_lossy().replace('\\', "/");
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .args(["log", "-1", "--format=%H %cd", "--date=format:%Y%m%d", "--"])
        .arg(&rel)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let line = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let (commit, updated_on) = line.split_once(' ')?;
    let commits_since = git_rev_list_count(repo_root, &[format!("{commit}..HEAD")])?;
    Some((updated_on.to_string(), commits_since))
}

fn git_commit_count_since_date(repo_root: &Path, ymd: &str) -> Option<usize> {
    let since = git_since_arg(ymd)?;
    git_rev_list_count(
        repo_root,
        &["--since".to_string(), since, "HEAD".to_string()],
    )
}

fn git_rev_list_count(repo_root: &Path, args: &[String]) -> Option<usize> {
    let mut cmd = std::process::Command::new("git");
    cmd.arg("-C").arg(repo_root).arg("rev-list").arg("--count");
    for arg in args {
        cmd.arg(arg);
    }
    let out = cmd.output().ok()?;
    if !out.status.success() {
        return None;
    }
    String::from_utf8_lossy(&out.stdout).trim().parse().ok()
}

fn git_since_arg(ymd: &str) -> Option<String> {
    if ymd.len() != 8 || !ymd.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some(format!("{}-{}-{}", &ymd[..4], &ymd[4..6], &ymd[6..8]))
}

fn age_days(today: &str, ymd: &str) -> Option<i64> {
    Some(ymd_to_days(today)? - ymd_to_days(ymd)?)
}

fn expected_generated_files(
    owox_dir: &Path,
    repo_root: &Path,
    canon: &owox_core::Canon,
) -> Vec<(std::path::PathBuf, owox_core::GeneratedFile)> {
    let targets: Vec<(String, String)> = if canon.targets.entries.is_empty() {
        vec![("codex".to_string(), ".".to_string())]
    } else {
        canon
            .targets
            .entries
            .iter()
            .map(|target| (target.name.clone(), target.out_dir.clone()))
            .collect()
    };
    let registered = owox_core::registered_skills(owox_dir, repo_root).unwrap_or_default();
    let commands = owox_core::command_skills(owox_dir).unwrap_or_default();
    let mut skills = registered;
    skills.extend(commands);
    let mut out = Vec::new();
    for (target_name, out_dir) in targets {
        let Some(target) = owox_core::find(&target_name) else {
            continue;
        };
        let root = repo_root.join(out_dir);
        for file in target.generate(canon) {
            out.push((root.join(&file.path), file));
        }
        for file in target.generate_skills(&skills) {
            out.push((root.join(&file.path), file));
        }
    }
    out
}

#[cfg(unix)]
fn file_is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .map(|meta| meta.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn file_is_executable(_path: &Path) -> bool {
    true
}

fn untraced_canon_change_findings(repo_root: &Path, refs: &ReferenceScan) -> Vec<GardeningFinding> {
    let base = crate::files::main_merge_base(repo_root);
    let changed = crate::files::changed_files_since(repo_root, &base.rev);
    let canon_changes: Vec<_> = changed
        .into_iter()
        .filter(|file| file.kind.is_canon_surface())
        .collect();
    if canon_changes.is_empty() || refs.summary.requirement_refs + refs.summary.decision_refs > 0 {
        return Vec::new();
    }
    canon_changes
        .into_iter()
        .map(|file| GardeningFinding {
            kind: "untraced-canon-change".to_string(),
            severity: "advisory",
            subject: file.path,
            detail: "canon or docs changed without an owox requirement or decision reference"
                .to_string(),
        })
        .collect()
}

fn gardening_json(findings: &[GardeningFinding]) -> serde_json::Value {
    let mut by_kind: BTreeMap<String, usize> = BTreeMap::new();
    let mut failed = 0usize;
    let mut advisory = 0usize;
    for finding in findings {
        *by_kind.entry(finding.kind.clone()).or_default() += 1;
        if finding.severity == "failed" {
            failed += 1;
        } else {
            advisory += 1;
        }
    }
    serde_json::json!({
        "summary": {
            "total": findings.len(),
            "failed": failed,
            "advisory": advisory,
            "by_kind": by_kind,
        },
        "findings": findings.iter().map(|finding| {
            serde_json::json!({
                "kind": finding.kind.clone(),
                "severity": finding.severity,
                "subject": finding.subject.clone(),
                "detail": finding.detail.clone(),
            })
        }).collect::<Vec<_>>(),
    })
}

fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = y - if m <= 2 { 1 } else { 0 };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = m as i64 + if m > 2 { -3 } else { 9 };
    let doy = (153 * mp + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn render_codebase_context_body(
    index: &crate::cache::CodebaseIndex,
    related_to_current_diff: &[String],
    mission: crate::cache::Mission,
    cache: &CodebaseCacheStatus,
) -> String {
    let mut out = String::from("# Codebase context\n\n");
    match mission {
        crate::cache::Mission::Kickoff => out.push_str(
            "Kickoff mission is active. Start from entrypoints and config before editing.\n\n",
        ),
        crate::cache::Mission::Review => out.push_str(
            "Review mission is active. Use this only when the diff map is not enough to place the change.\n\n",
        ),
        crate::cache::Mission::Verify => out.push_str(
            "Verify mission is active. Use this to find test and build entrypoints.\n\n",
        ),
        crate::cache::Mission::Handoff => out.push_str(
            "Handoff mission is active. Use this to point the next session at the right areas.\n\n",
        ),
        crate::cache::Mission::Work => {}
    }
    out.push_str("Root:\n");
    out.push_str(&format!("- kind: {}\n", index.root_kind));
    if !index.package_files.is_empty() {
        out.push_str("- package files:\n");
        for file in &index.package_files {
            out.push_str(&format!("  - {file}\n"));
        }
    }
    out.push('\n');
    if !index.areas.is_empty() {
        out.push_str("## Areas\n\n");
        for area in &index.areas {
            out.push_str(&format!("- {} [{}]: {}\n", area.path, area.kind, area.role));
            for evidence in &area.evidence {
                out.push_str(&format!("  - evidence: {evidence}\n"));
            }
        }
        out.push('\n');
    }
    if !index.entrypoints.is_empty() {
        out.push_str("## Entrypoints\n\n");
        for entry in &index.entrypoints {
            out.push_str(&format!("- {entry}\n"));
        }
        out.push('\n');
    }
    if !index.checks.is_empty() {
        out.push_str("## Checks\n\n");
        for check in &index.checks {
            out.push_str(&format!("- {check}\n"));
        }
        out.push('\n');
    }
    if !index.generated_or_external.is_empty() {
        out.push_str("## Generated or external\n\n");
        for path in &index.generated_or_external {
            out.push_str(&format!("- {path}\n"));
        }
        out.push('\n');
    }
    if !related_to_current_diff.is_empty() {
        out.push_str("## Related to current diff\n\n");
        for path in related_to_current_diff {
            out.push_str(&format!("- {path}\n"));
        }
        out.push('\n');
    }
    out.push_str("Cache:\n");
    out.push_str(&format!("- stale: {}\n", cache.stale));
    out.push_str(&format!("- refreshed: {}\n", cache.refreshed));
    for reason in &cache.reasons {
        out.push_str(&format!("- refresh reason: {reason}\n"));
    }
    if let Some(head) = &index.git_head {
        out.push_str(&format!("- git head: {}\n", short_id(head)));
    }
    out.push_str(&format!("- generated on: {}\n", index.generated_on));
    out
}

/// 文脈地図を Markdown へ描画する。作業/パスごとに「読む先 + 適用ルール」を示す。
///
/// 末尾に推定トークン数を 1 行足し、owox が注入する情報量を毎回数値で見せる
/// (旗「最小コンテキスト」の測定可能化)。同じ本文を秘密走査し、当たれば警告行を足す
/// (read 専用ナビなので block せず気づかせる。`docs/decisions/20260614-Phase7-測定可視化とブランド検証.md`)。
fn render_context(context: &owox_core::Context) -> String {
    let mut out = if context.entries.is_empty() {
        String::from("# Context map\n\nNo entries yet.\n")
    } else {
        let mut out = String::from("# Context map\n\n");
        for entry in &context.entries {
            out.push_str(&format!("## {}\n\n", entry.scope));
            if !entry.reads.is_empty() {
                out.push_str("Read:\n");
                for r in &entry.reads {
                    out.push_str(&format!("- {r}\n"));
                }
            }
            if !entry.notes.is_empty() {
                out.push_str("Apply:\n");
                for n in &entry.notes {
                    out.push_str(&format!("- {n}\n"));
                }
            }
            out.push('\n');
        }
        out
    };

    for finding in owox_core::secret::scan(&out) {
        out.push_str(&format!(
            "\n> Secret warning [{}]: {}\n",
            finding.id, finding.detail
        ));
    }
    out.push_str(&format!(
        "\nEstimated size: ~{} tokens.\n",
        owox_core::tokens::estimate_tokens(&out)
    ));
    out
}

fn mission_preview(
    owox_dir: &Path,
    repo_root: &Path,
    mission: crate::cache::Mission,
) -> Option<String> {
    let decisions = list_decisions(owox_dir).unwrap_or_default();
    let tasks = list_tasks(owox_dir).unwrap_or_default();
    let requirements = list_requirements(owox_dir).unwrap_or_default();
    let canon = owox_core::load_canon(owox_dir).ok();
    if mission == crate::cache::Mission::Kickoff {
        let fallback = owox_core::Canon::default();
        let canon = canon.as_ref().unwrap_or(&fallback);
        let questions = build_kickoff_questions(owox_dir, repo_root, canon, &decisions, &tasks);
        return Some(render_kickoff_next(&questions));
    }
    let axes = canon
        .as_ref()
        .map(|canon| canon.profile.resolve().unwrap_or_default())
        .unwrap_or_default();
    Some(render_next(
        &decisions,
        &tasks,
        &requirements,
        &[],
        &[],
        &[],
        &[],
        axes,
        AutoApproval {
            profile: false,
            session: false,
        },
        mission,
    ))
}

fn render_kickoff_context(
    repo_root: &Path,
    owox_dir: &Path,
    canon: &owox_core::Canon,
    decisions: &[owox_core::Decision],
    tasks: &[Task],
) -> String {
    let questions = build_kickoff_questions(owox_dir, repo_root, canon, decisions, tasks);
    let (files, has_quality_layers, has_version_tags) = detect_inputs(repo_root, owox_dir);
    let signals = owox_core::DetectSignals {
        files: &files,
        has_quality_layers,
        has_version_tags,
    };
    let profile_declared = profile_declared_at(owox_dir);
    let profile_draft = owox_core::detect_profile(&signals);
    let canon_draft = owox_core::detect_canon_draft(&signals);
    let checks = detect_checks(&detect_package_files(&files));
    let open_decisions = decisions
        .iter()
        .filter(|decision| decision.status == DecisionStatus::Open)
        .count();
    let mut out = String::from("# Kickoff context\n\n");
    out.push_str(
        "Kickoff mission is active. Use this map to decide setup, not to start implementation.\n\n",
    );
    out.push_str("## Current state\n\n");
    out.push_str(&format!("- phase: {}\n", canon.state.phase.as_str()));
    out.push_str(&format!("- profile declared: {}\n", profile_declared));
    out.push_str(&format!("- open decisions: {open_decisions}\n"));
    out.push_str(&format!("- tasks already recorded: {}\n", tasks.len()));
    out.push_str(&format!(
        "- verify checks declared: {}\n\n",
        canon.verify.checks.len()
    ));

    out.push_str("## Unresolved setup pressure\n\n");
    if questions.is_empty() {
        out.push_str("- no unresolved kickoff question remains\n");
        out.push_str("- switch the mission back to work when ready\n\n");
    } else {
        out.push_str(&format!("- unresolved questions: {}\n", questions.len()));
        for question in questions.iter().take(5) {
            out.push_str(&format!(
                "- {} / {}: {} ({})\n",
                question.stage, question.item, question.recommendation, question.decider
            ));
            out.push_str(&format!("  reason: {}\n", question.reason));
        }
        if questions.len() > 5 {
            out.push_str(&format!("- ... and {} more\n", questions.len() - 5));
        }
        out.push('\n');
    }

    out.push_str("## Existing code signals\n\n");
    if profile_declared {
        let axes = canon.profile.resolve().unwrap_or_default();
        out.push_str(&format!(
            "- project nature: declared as {} / {} / {} / {}\n",
            axes.requirements_shape.as_str(),
            axes.prioritization.as_str(),
            axes.delivery.as_str(),
            axes.architecture.as_str()
        ));
    } else {
        out.push_str(&format!(
            "- project nature draft: {}\n",
            profile_draft
                .suggested_preset
                .clone()
                .unwrap_or_else(|| "no preset match".to_string())
        ));
        out.push_str(&format!(
            "  evidence: {}; {}; {}; {}\n",
            profile_draft.requirements_shape.evidence,
            profile_draft.prioritization.evidence,
            profile_draft.delivery.evidence,
            profile_draft.architecture.evidence
        ));
    }
    if canon_draft.is_empty() {
        out.push_str("- guardrail draft: no strong signal from existing code\n");
    } else {
        out.push_str(&format!(
            "- guardrail draft: {} layer, {} boundary, {} irreversible candidate\n",
            canon_draft.layers.len(),
            canon_draft.boundaries.len(),
            canon_draft.irreversible.len()
        ));
        for evidence in kickoff_canon_evidence(&canon_draft).into_iter().take(3) {
            out.push_str(&format!("  evidence: {evidence}\n"));
        }
    }
    if canon.verify.checks.is_empty() {
        if checks.is_empty() {
            out.push_str("- verify entry draft: no common test entry detected\n");
        } else {
            out.push_str(&format!("- verify entry draft: {}\n", checks.join(", ")));
        }
    } else {
        out.push_str(&format!(
            "- verify entry: declared as {}\n",
            canon
                .verify
                .checks
                .iter()
                .map(|check| check.command.clone())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    out.push('\n');

    out.push_str("## Read next\n\n");
    out.push_str("- call next to get the single next setup decision\n");
    out.push_str("- call context with scope codebase when repo shape matters\n");
    out.push_str("- call context with scope diff when changed canon or docs matter\n");
    out
}

fn kickoff_canon_evidence(draft: &owox_core::CanonDraft) -> Vec<String> {
    let mut evidence = Vec::new();
    for layer in &draft.layers {
        evidence.push(layer.evidence.clone());
    }
    for boundary in &draft.boundaries {
        evidence.push(boundary.evidence.clone());
    }
    for irreversible in &draft.irreversible {
        evidence.push(irreversible.evidence.clone());
    }
    evidence.sort();
    evidence.dedup();
    evidence
}

fn kickoff_status_json(
    owox_dir: &Path,
    repo_root: &Path,
    canon: &owox_core::Canon,
    decisions: &[owox_core::Decision],
    tasks: &[Task],
) -> serde_json::Value {
    let questions = build_kickoff_questions(owox_dir, repo_root, canon, decisions, tasks);
    serde_json::json!({
        "unresolved": questions.len(),
        "ai_drafts": questions.iter().filter(|q| q.decider == "ai").count(),
        "human_decisions": questions.iter().filter(|q| q.decider == "human").count(),
        "next_question": questions.first().map(kickoff_question_json),
        "ready_to_return": questions.is_empty(),
        "canonicalization_candidates": kickoff_candidates_json(owox_dir, repo_root, canon, tasks),
    })
}

fn kickoff_candidates_json(
    owox_dir: &Path,
    repo_root: &Path,
    canon: &owox_core::Canon,
    tasks: &[Task],
) -> Vec<serde_json::Value> {
    let (files, has_quality_layers, has_version_tags) = detect_inputs(repo_root, owox_dir);
    let profile_draft = owox_core::detect_profile(&owox_core::DetectSignals {
        files: &files,
        has_quality_layers,
        has_version_tags,
    });
    let canon_draft = owox_core::detect_canon_draft(&owox_core::DetectSignals {
        files: &files,
        has_quality_layers,
        has_version_tags,
    });
    let checks = detect_checks(&detect_package_files(&files));
    let thin_guardrails = canon.quality.layers.is_empty()
        && canon.quality.boundaries.is_empty()
        && canon.rules.irreversible.is_empty();
    let mut out = Vec::new();
    if !profile_declared_at(owox_dir) {
        let reason = [
            profile_draft.requirements_shape.evidence,
            profile_draft.prioritization.evidence,
            profile_draft.delivery.evidence,
            profile_draft.architecture.evidence,
        ]
        .join("; ");
        out.push(serde_json::json!({
            "kind": "profile",
            "route": "profile.set",
            "summary": format!(
                "Declare the project nature as {}",
                profile_draft
                    .suggested_preset
                    .clone()
                    .unwrap_or_else(|| "the detected draft".to_string())
            ),
            "reason": reason,
        }));
    }
    if thin_guardrails && !canon_draft.is_empty() {
        out.push(serde_json::json!({
            "kind": "guardrails",
            "route": "canon.detect draft review",
            "summary": "Review the detected guardrails and adopt the needed ones",
            "reason": format!(
                "{} layer, {} boundary, {} irreversible candidate",
                canon_draft.layers.len(),
                canon_draft.boundaries.len(),
                canon_draft.irreversible.len()
            ),
        }));
    }
    if canon.verify.checks.is_empty() && !checks.is_empty() {
        out.push(serde_json::json!({
            "kind": "verify",
            "route": "config.toml [[verify.checks]]",
            "summary": "Declare the first verification checks",
            "reason": format!("Detected: {}", checks.join(", ")),
        }));
    }
    if tasks.is_empty() {
        out.push(serde_json::json!({
            "kind": "task",
            "route": "task.create",
            "summary": "Record the first task before leaving kickoff",
            "reason": "No task has been created yet",
        }));
    }
    out
}

fn build_kickoff_questions(
    owox_dir: &Path,
    repo_root: &Path,
    canon: &owox_core::Canon,
    decisions: &[owox_core::Decision],
    tasks: &[Task],
) -> Vec<KickoffQuestion> {
    let mut out = Vec::new();
    for decision in decisions
        .iter()
        .filter(|decision| decision.status == DecisionStatus::Open)
    {
        out.push(KickoffQuestion {
            stage: "入口確認",
            item: format!("未決の判断 {} を確定する", decision.title),
            recommendation: "いま人間が確定".to_string(),
            reason: if decision.rationale.trim().is_empty() {
                "open decision が残ると後続が止まりやすい".to_string()
            } else {
                decision.rationale.trim().to_string()
            },
            decider: "human",
            options: vec![
                "adopt".to_string(),
                "reject".to_string(),
                "defer".to_string(),
            ],
        });
    }

    if !profile_declared_at(owox_dir) {
        let (files, has_quality_layers, has_version_tags) = detect_inputs(repo_root, owox_dir);
        let draft = owox_core::detect_profile(&owox_core::DetectSignals {
            files: &files,
            has_quality_layers,
            has_version_tags,
        });
        let recommendation = draft
            .suggested_preset
            .clone()
            .unwrap_or_else(|| "clean-arch-app".to_string());
        let reason = [
            draft.requirements_shape.evidence,
            draft.prioritization.evidence,
            draft.delivery.evidence,
            draft.architecture.evidence,
        ]
        .join("; ");
        out.push(KickoffQuestion {
            stage: "作業の型",
            item: "project nature を決める".to_string(),
            recommendation,
            reason,
            decider: "human",
            options: owox_core::builtin_bundle_names()
                .iter()
                .map(|name| (*name).to_string())
                .collect(),
        });
    }

    let (files, has_quality_layers, has_version_tags) = detect_inputs(repo_root, owox_dir);
    let canon_draft = owox_core::detect_canon_draft(&owox_core::DetectSignals {
        files: &files,
        has_quality_layers,
        has_version_tags,
    });
    let thin_guardrails = canon.quality.layers.is_empty()
        && canon.quality.boundaries.is_empty()
        && canon.rules.irreversible.is_empty();
    if thin_guardrails && !canon_draft.is_empty() {
        out.push(KickoffQuestion {
            stage: "安全境界",
            item: "初期ガードレール案を採るか決める".to_string(),
            recommendation: "検出案を初期値として採る".to_string(),
            reason: "既存コードから層・境界・不可逆操作の案が取れている".to_string(),
            decider: "human",
            options: vec![
                "検出案を採る".to_string(),
                "一部だけ採る".to_string(),
                "手で決める".to_string(),
            ],
        });
    }

    let checks = detect_checks(&detect_package_files(&crate::files::list_repo_files(
        repo_root,
    )));
    if canon.verify.checks.is_empty() && !checks.is_empty() {
        out.push(KickoffQuestion {
            stage: "作業の型",
            item: "最初の検証入口を決める".to_string(),
            recommendation: checks[0].clone(),
            reason: "repo から既存の test / build 入口が見えている".to_string(),
            decider: "human",
            options: checks,
        });
    }

    if tasks.is_empty() {
        out.push(KickoffQuestion {
            stage: "初期 task",
            item: "最初の task 分割".to_string(),
            recommendation: "AI仮決定".to_string(),
            reason: "repo 構造と未決一覧から初手は機械的に切りやすい".to_string(),
            decider: "ai",
            options: vec!["AI仮決定".to_string(), "人間が先に決める".to_string()],
        });
    }

    out
}

fn render_kickoff_next(questions: &[KickoffQuestion]) -> String {
    let mut out = String::from("# What to decide next\n\n");
    out.push_str(
        "Kickoff mission is active. Resolve one setup decision before implementation.\n\n",
    );
    let Some(question) = questions.first() else {
        out.push_str("No unresolved kickoff decision remains.\n\n");
        out.push_str("Decide whether to switch the mission back to work.\n");
        return out;
    };
    out.push_str(&format!("Stage: {}\n", question.stage));
    out.push_str(&format!("Decide: {}\n", question.item));
    out.push_str(&format!("Recommended: {}\n", question.recommendation));
    out.push_str(&format!("Reason: {}\n", question.reason));
    out.push_str(&format!("Decider: {}\n", question.decider));
    out.push_str("Options:\n");
    for option in &question.options {
        out.push_str(&format!("- {option}\n"));
    }
    out.push_str("Next: waiting for one answer\n");
    out
}

fn kickoff_question_json(question: &KickoffQuestion) -> serde_json::Value {
    serde_json::json!({
        "stage": question.stage,
        "item": question.item,
        "recommendation": question.recommendation,
        "reason": question.reason,
        "decider": question.decider,
        "options": question.options,
    })
}

fn profile_declared_at(owox_dir: &Path) -> bool {
    owox_dir.join("profile.toml").exists()
}

/// 次に手を付けるもの = 未決 (status=open の来歴) + ready タスク + trace が要る要件を描画する。
///
/// trace が要る要件 = accepted だが受け入れ基準が無い or 検証 link が欠けるもの。
/// 検査を実行せず静的に分かる信号で先回りする (`docs/decisions/20260614-Phase6-要件完了の機械判定.md`)。
fn render_next(
    decisions: &[owox_core::Decision],
    tasks: &[Task],
    requirements: &[owox_core::Requirement],
    decay: &[DecayFinding],
    routines: &[owox_core::RoutineSuggestion],
    gardening: &[GardeningFinding],
    glossary_suggestions: &[owox_core::GlossarySuggestion],
    axes: owox_core::Axes,
    auto: AutoApproval,
    mission: crate::cache::Mission,
) -> String {
    let open: Vec<_> = decisions
        .iter()
        .filter(|d| d.status == DecisionStatus::Open)
        .collect();
    // 後追いキュー: 自動承認したが人間がまだ確認していない来歴。
    let auto_pending: Vec<_> = decisions
        .iter()
        .filter(|d| d.status == DecisionStatus::Adopted && d.auto_approved && !d.confirmed)
        .collect();
    let ready: Vec<_> = tasks.iter().filter(|t| is_ready(t, tasks)).collect();
    let untraced: Vec<_> = requirements.iter().filter(|r| r.needs_trace()).collect();
    // 理想先行: 並べ替えが要る (優先度未設定の) accepted 要件。prioritization=ideal-first の時だけ。
    let unprioritized: Vec<_> = if axes.ideal_first_active() {
        requirements
            .iter()
            .filter(|r| r.status == RequirementStatus::Accepted && r.priority.is_none())
            .collect()
    } else {
        Vec::new()
    };

    let mut out = String::from("# What to decide next\n\n");
    match mission {
        crate::cache::Mission::Kickoff => out.push_str(
            "Kickoff mission is active. Prioritize unresolved setup decisions before implementation.\n\n",
        ),
        crate::cache::Mission::Review => {
            out.push_str("Review mission is active. Prioritize inspection and findings.\n\n")
        }
        crate::cache::Mission::Verify => {
            out.push_str("Verify mission is active. Prioritize checks and completion evidence.\n\n")
        }
        crate::cache::Mission::Handoff => {
            out.push_str("Handoff mission is active. Prioritize verified state and open decisions.\n\n")
        }
        crate::cache::Mission::Work => {}
    }
    if open.is_empty()
        && ready.is_empty()
        && untraced.is_empty()
        && decay.is_empty()
        && routines.is_empty()
        && gardening.is_empty()
        && glossary_suggestions.is_empty()
        && unprioritized.is_empty()
        && auto_pending.is_empty()
        && !auto.active()
    {
        out.push_str(
            "Nothing is open, no task is ready, every accepted requirement has a verification trace, and nothing is decaying.\n",
        );
        return out;
    }
    // 自動承認が有効な時は最初に知らせる。同意源で文言を分ける (profile 由来は永続・session 由来は session 限り)。
    if auto.profile {
        out.push_str("## Automatic approval is on\n\n");
        out.push_str(
            "This project's nature (flat architecture) keeps automatic approval on for as long as the profile stays flat. You may approve non-guarded gates yourself with gate.auto_approve.\n\n",
        );
    } else if auto.session {
        out.push_str("## Automatic approval is on\n\n");
        out.push_str(
            "You may approve non-guarded gates yourself with gate.auto_approve this session. It closes at the next session start. Turn it off with gate.auto_disable.\n\n",
        );
    }
    // 後追いキュー: 自動承認した判断を人間が確認 or 差し戻す導線。
    if !auto_pending.is_empty() {
        out.push_str("## Auto-approved, awaiting the human's confirmation\n\n");
        out.push_str(
            "These were approved automatically while automatic approval was on. The human confirms one with gate.confirm or undoes it with gate.revert.\n",
        );
        for d in &auto_pending {
            out.push_str(&format!("- {} ({})", d.title, d.id));
            if !d.rationale.trim().is_empty() {
                out.push_str(&format!(": {}", d.rationale.trim()));
            }
            out.push('\n');
        }
        out.push('\n');
    }
    if !open.is_empty() {
        out.push_str("## Open decisions\n\n");
        for d in open {
            // 自律度を明示して承認手段の取り違えを防ぐ。guarded は人間のみ、非 guarded は窓が開いていれば自動承認可。
            let gate_tag = match owox_core::gate_autonomy(d) {
                owox_core::Autonomy::Guarded => {
                    " [guarded: only a human approves, via gate.approve]"
                }
                _ => {
                    " [non-guarded: approve with gate.auto_approve while automatic approval is on, otherwise gate.approve]"
                }
            };
            out.push_str(&format!("- {} ({}){}", d.title, d.id, gate_tag));
            if !d.rationale.trim().is_empty() {
                out.push_str(&format!(": {}", d.rationale.trim()));
            }
            out.push('\n');
        }
        out.push('\n');
    }
    if !ready.is_empty() {
        out.push_str("## Ready tasks\n\n");
        for t in ready {
            // 段階化: stage を添える (delivery=phased の時だけ)。
            match (axes.phased_active(), &t.stage) {
                (true, Some(s)) => {
                    out.push_str(&format!("- {} ({}) [stage: {}]\n", t.title, t.id, s))
                }
                _ => out.push_str(&format!("- {} ({})\n", t.title, t.id)),
            }
        }
        out.push('\n');
    }
    // 理想先行: 人間が優先順位を並べる行為を促す (AI は提案まで)。
    if !unprioritized.is_empty() {
        out.push_str("## Requirements to prioritize\n\n");
        out.push_str(
            "A human ranks these accepted requirements by ideal priority; set priority with requirement.update.\n\n",
        );
        for r in &unprioritized {
            out.push_str(&format!("- {} ({})\n", r.title, r.id));
        }
        out.push('\n');
    }
    // クリーンアーキ: 層別の進行度 (architecture=layered の時だけ)。
    if axes.layered_active() {
        let progress = owox_core::layer_progress(requirements);
        if !progress.is_empty() {
            out.push_str("## Layer progress\n\n");
            for (layer, total, traced) in progress {
                out.push_str(&format!(
                    "- {layer}: {traced}/{total} requirements traced\n"
                ));
            }
            out.push('\n');
        }
    }
    if !untraced.is_empty() {
        out.push_str("## Requirements needing a verification trace\n\n");
        for r in untraced {
            let detail = if r.criteria.is_empty() {
                "no acceptance criteria yet".to_string()
            } else {
                format!("{} criteria without a verification link", r.unlinked())
            };
            out.push_str(&format!("- {} ({}): {}\n", r.title, r.id, detail));
        }
        out.push('\n');
    }
    if !decay.is_empty() {
        out.push_str(&render_decay(decay));
    }
    if !gardening.is_empty() {
        out.push_str(&render_gardening(gardening));
    }
    if !glossary_suggestions.is_empty() {
        out.push_str(&render_glossary_suggestions(glossary_suggestions));
    }
    if !routines.is_empty() {
        out.push_str(&render_routines(routines));
    }
    out
}

/// 育てられる手順を描く。頻出する隣接列を上限まで挙げ、スキルライフサイクルへ案内する (advisory)。
fn render_routines(routines: &[owox_core::RoutineSuggestion]) -> String {
    const SHOWN: usize = 5;
    let mut out = String::from("## Routines you could grow into a skill\n\n");
    out.push_str(
        "These step sequences repeat often. Consider capturing one as a skill: write SKILL.md + a script under .owox/skills/, then skill.register (tests gate) and skill.promote (human).\n\n",
    );
    for r in routines.iter().take(SHOWN) {
        out.push_str(&format!(
            "- {} (seen {}x, {})\n",
            r.sequence.join(" → "),
            r.occurrences,
            r.kind.as_str()
        ));
        if let Some(script) = &r.suggested_script {
            out.push_str(&format!("  script: {script}\n"));
        }
    }
    if routines.len() > SHOWN {
        out.push_str(&format!("- … and {} more\n", routines.len() - SHOWN));
    }
    out.push('\n');
    out
}

fn render_gardening(gardening: &[GardeningFinding]) -> String {
    const SHOWN: usize = 5;
    let mut out = String::from("## Gardening candidates\n\n");
    for finding in gardening.iter().take(SHOWN) {
        out.push_str(&format!(
            "- {} [{}]: {}\n",
            finding.subject, finding.kind, finding.detail
        ));
    }
    if gardening.len() > SHOWN {
        out.push_str(&format!("- … and {} more\n", gardening.len() - SHOWN));
    }
    out.push('\n');
    out
}

/// 腐敗警告を要約して描く。context を汚さぬよう種別ごとに件数を集計し、項目は上限まで。
fn render_decay(decay: &[DecayFinding]) -> String {
    const SHOWN: usize = 8;
    let mut out = String::from("## Decay warnings\n\n");

    // 種別ごとの件数を 1 行で示す (全体像)。
    let mut counts: std::collections::BTreeMap<&str, usize> = std::collections::BTreeMap::new();
    for f in decay {
        *counts.entry(f.kind).or_default() += 1;
    }
    let summary: Vec<String> = counts.iter().map(|(k, n)| format!("{k}: {n}")).collect();
    out.push_str(&format!(
        "{} finding(s) — {}\n\n",
        decay.len(),
        summary.join(", ")
    ));

    // 個別項目は上限まで (全件は出さず context を汚さない)。
    for f in decay.iter().take(SHOWN) {
        out.push_str(&format!("- {} ({}): {}\n", f.kind, f.subject, f.detail));
    }
    if decay.len() > SHOWN {
        out.push_str(&format!(
            "- … and {} more (see verify.run data.decay for the full list)\n",
            decay.len() - SHOWN
        ));
    }
    out.push('\n');
    out
}

/// 生成済み inputSchema から意味に影響しない構造定型を除く
/// (`docs/decisions/20260620-MCPコンテキスト削減.md`)。
///
/// rmcp/schemars は生成時に title/default を抑制できない (SchemaSettings 内部固定) ため
/// 生成後の JSON を後処理する。router の全 route を回すので tool 追加にも自動追従する。
/// title・default の除去は引数解釈に無関係で完全に安全 (デシリアライズは serde default が担い
/// schema と独立)。`$schema` は欠落で厳格クライアントが draft 推定を失敗する理論懸念があり、
/// 実機 Codex 検証 (target-validate) を通すまで残す (見直し条件: 受理確認後に除去)。
fn clean_schemas(mut router: ToolRouter<OwoxServer>) -> ToolRouter<OwoxServer> {
    for route in router.map.values_mut() {
        let schema = std::sync::Arc::make_mut(&mut route.attr.input_schema);
        schema.remove("title");
        // properties 配下の null / 空配列 default を除く (serde default が担う重複)。
        if let Some(serde_json::Value::Object(props)) = schema.get_mut("properties") {
            for prop in props.values_mut() {
                let serde_json::Value::Object(field) = prop else {
                    continue;
                };
                let drop_default = match field.get("default") {
                    Some(serde_json::Value::Null) => true,
                    Some(serde_json::Value::Array(a)) => a.is_empty(),
                    _ => false,
                };
                if drop_default {
                    field.remove("default");
                }
            }
        }
    }
    router
}

/// `owox serve [path]` を捌く。
///
/// path 省略時はカレントディレクトリから上方へ `.owox` を探す。
/// Codex は MCP サーバを起動時の作業ディレクトリで立てるため、サブディレクトリ
/// 起動でも正本を見つけられる。生成 config はパスを焼かず serve だけ渡す。
pub fn run(args: &[String]) -> ExitCode {
    let start = args
        .first()
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    // 明示パスが渡された時はその直下の `.owox` を、無ければ上方探索で解決する。
    let owox_dir = if args.is_empty() {
        find_owox_dir(&start).unwrap_or_else(|| start.join(".owox"))
    } else {
        start.join(".owox")
    };

    // stdin/stdout の非同期入出力は blocking pool 経由 (io-std)。IO driver は不要。
    // rmcp 内部の通信処理がタイマーを使うため time だけ有効化する。
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
    {
        Ok(rt) => rt,
        Err(err) => {
            eprintln!("owox serve: runtime を起こせない: {err}");
            return ExitCode::FAILURE;
        }
    };

    runtime.block_on(async move {
        let server = OwoxServer {
            owox_dir,
            tool_router: clean_schemas(OwoxServer::tool_router()),
        };
        let running = match server.serve(stdio()).await {
            Ok(running) => running,
            Err(err) => {
                eprintln!("owox serve: 初期化に失敗: {err}");
                return ExitCode::FAILURE;
            }
        };
        // クライアント (Codex) が切るまで待つ。
        if let Err(err) = running.waiting().await {
            eprintln!("owox serve: 異常終了: {err}");
            return ExitCode::FAILURE;
        }
        ExitCode::SUCCESS
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use std::process::Command;

    fn temp_git_repo(name: &str) -> std::path::PathBuf {
        use std::sync::atomic::{AtomicU32, Ordering};
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let repo = std::env::temp_dir().join(format!("{name}-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&repo);
        std::fs::create_dir_all(&repo).unwrap();
        let status = Command::new("git")
            .arg("-C")
            .arg(&repo)
            .args(["init"])
            .status()
            .unwrap();
        assert!(status.success());
        repo
    }

    fn git_commit(repo: &Path, rel: &str, contents: &str, when: &str, message: &str) {
        let path = repo.join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&path, contents).unwrap();
        let add = Command::new("git")
            .arg("-C")
            .arg(repo)
            .args(["add", rel])
            .status()
            .unwrap();
        assert!(add.success());
        let commit = Command::new("git")
            .arg("-C")
            .arg(repo)
            .args(["commit", "-m", message])
            .env("GIT_AUTHOR_DATE", when)
            .env("GIT_COMMITTER_DATE", when)
            .env("GIT_AUTHOR_NAME", "owox")
            .env("GIT_AUTHOR_EMAIL", "owox@example.com")
            .env("GIT_COMMITTER_NAME", "owox")
            .env("GIT_COMMITTER_EMAIL", "owox@example.com")
            .status()
            .unwrap();
        assert!(commit.success());
    }

    #[test]
    fn extract_references_trims_trailing_punctuation() {
        let refs = extract_references(
            "see owox:req:20260625-a#2, owox:dec:20260625-b. and owox:req:20260625-c!",
        );
        assert_eq!(refs.len(), 3);
        match &refs[0] {
            ReferenceTarget::Requirement { id, criterion } => {
                assert_eq!(id, "20260625-a");
                assert_eq!(*criterion, Some(2));
            }
            _ => panic!("unexpected first ref"),
        }
        match &refs[1] {
            ReferenceTarget::Decision { id } => assert_eq!(id, "20260625-b"),
            _ => panic!("unexpected second ref"),
        }
    }

    #[test]
    fn parse_reference_query_accepts_requirement_and_decision_forms() {
        match parse_reference_query("owox:req:20260625-a#3").unwrap() {
            ReferenceTarget::Requirement { id, criterion } => {
                assert_eq!(id, "20260625-a");
                assert_eq!(criterion, Some(3));
            }
            _ => panic!("requirement expected"),
        }
        match parse_reference_query("owox:dec:20260625-b").unwrap() {
            ReferenceTarget::Decision { id } => assert_eq!(id, "20260625-b"),
            _ => panic!("decision expected"),
        }
        assert!(parse_reference_query("bad").is_none());
    }

    #[test]
    fn reference_lookup_lists_usage_files_and_missing_criterion_candidates() {
        let repo = temp_git_repo("owox-reference-lookup");
        std::fs::create_dir_all(repo.join("src")).unwrap();
        std::fs::create_dir_all(repo.join("docs")).unwrap();
        std::fs::write(repo.join("src/lib.rs"), "// owox:req:req-1\n").unwrap();
        std::fs::write(repo.join("docs/trace.md"), "owox:req:req-1#2\n").unwrap();

        let requirements = vec![owox_core::Requirement {
            id: "req-1".to_string(),
            title: "traceability".to_string(),
            status: owox_core::RequirementStatus::Accepted,
            statement: String::new(),
            criteria: vec![
                owox_core::AcceptanceCriterion {
                    id: 1,
                    title: String::new(),
                    given: String::new(),
                    when: String::new(),
                    then: "first".to_string(),
                    verify: None,
                },
                owox_core::AcceptanceCriterion {
                    id: 2,
                    title: String::new(),
                    given: String::new(),
                    when: String::new(),
                    then: "second".to_string(),
                    verify: None,
                },
            ],
            links: owox_core::RequirementLinks::default(),
            supersedes: Vec::new(),
            priority: None,
            layer: None,
            stage: None,
            kind: None,
        }];
        let decisions = vec![owox_core::Decision {
            id: "dec-1".to_string(),
            title: "trace decision".to_string(),
            status: DecisionStatus::Open,
            rationale: String::new(),
            links: owox_core::DecisionLinks::default(),
            supersedes: Vec::new(),
            proposed_change: None,
            authorizes: Vec::new(),
            consumed: false,
            approval: None,
            auto_approved: false,
            confirmed: false,
        }];

        let found = build_reference_lookup_data(&repo, "owox:req:req-1", &requirements, &decisions)
            .expect("lookup parses");
        assert!(found.exists);
        assert_eq!(found.used_by.len(), 2);
        assert!(found.used_by.iter().any(|item| item.path == "src/lib.rs"));
        assert!(
            found
                .used_by
                .iter()
                .any(|item| item.path == "docs/trace.md")
        );

        let missing =
            build_reference_lookup_data(&repo, "owox:req:req-1#9", &requirements, &decisions)
                .expect("lookup parses");
        assert!(!missing.exists);
        assert!(
            missing
                .candidates
                .iter()
                .any(|item| item == "owox:req:req-1#1")
        );
        assert!(
            missing
                .candidates
                .iter()
                .any(|item| item == "owox:req:req-1#2")
        );
    }

    #[test]
    fn detect_codebase_areas_uses_mechanical_roles() {
        let files = vec![
            "Cargo.toml".to_string(),
            "crates/mcp/src/main.rs".to_string(),
            "crates/core/src/lib.rs".to_string(),
            "docs/requirements/x.md".to_string(),
        ];
        let areas = detect_codebase_areas(&files);
        assert!(areas.iter().any(|a| {
            a.path == "crates/mcp/src" && a.role == "Executable surface" && a.kind == "source"
        }));
        assert!(
            areas
                .iter()
                .any(|a| a.path == "crates/core/src" && a.role == "Library source")
        );
        assert!(
            areas
                .iter()
                .any(|a| a.path == "docs" && a.role == "Project docs")
        );
    }

    #[test]
    fn codebase_stale_reasons_detect_head_file_and_age() {
        let repo = std::env::temp_dir().join(format!("owox-codebase-stale-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&repo);
        std::fs::create_dir_all(repo.join("src")).unwrap();
        std::fs::write(repo.join("Cargo.toml"), "[package]\nname = \"x\"\n").unwrap();
        std::fs::write(repo.join("src/lib.rs"), "pub fn x() {}\n").unwrap();

        let index = crate::cache::CodebaseIndex {
            root_kind: "rust-crate".to_string(),
            package_files: vec!["Cargo.toml".to_string()],
            areas: Vec::new(),
            entrypoints: vec!["src/lib.rs".to_string()],
            checks: vec!["cargo test".to_string()],
            generated_or_external: Vec::new(),
            source_files: vec!["Cargo.toml".to_string(), "src/lib.rs".to_string()],
            git_head: Some("old-head".to_string()),
            generated_on: "20260601".to_string(),
        };

        std::fs::remove_file(repo.join("src/lib.rs")).unwrap();
        let reasons = codebase_stale_reasons(&repo, &index, Some("new-head"), "20260626");
        assert!(reasons.iter().any(|r| r == "git head changed"));
        assert!(
            reasons
                .iter()
                .any(|r| r == "evidence file missing: src/lib.rs")
        );
        assert!(reasons.iter().any(|r| r.contains("cache age")));
    }

    #[test]
    fn diff_context_body_suggests_codebase_when_needed() {
        let data = DiffContextData {
            base: crate::files::DiffBase {
                name: "merge-base(main, HEAD)".to_string(),
                rev: "1234567890".to_string(),
            },
            changed_files: vec![crate::files::ChangedFile {
                path: "weird/path.bin".to_string(),
                previous_path: None,
                status: crate::files::ChangeStatus::Modified,
                kind: crate::files::FileKind::Unknown,
            }],
            canon_changes: Vec::new(),
            reference_summary: ReferenceSummary::default(),
            review_hints: vec!["h".to_string()],
            gardening_hints: Vec::new(),
            needs_codebase: true,
            guidance: owox_core::DeliverySelection::default(),
            glossary_suggestions: Vec::new(),
        };
        let out = render_diff_context_body(&data, crate::cache::Mission::Work);
        assert!(out.contains("scope codebase"));
    }

    #[test]
    fn clean_schemas_strips_title_and_empty_defaults_but_keeps_schema() {
        let router = clean_schemas(OwoxServer::tool_router());
        assert!(!router.map.is_empty());
        for route in router.map.values() {
            let schema = route.attr.input_schema.as_ref();
            // title は全 route から消える。
            assert!(
                !schema.contains_key("title"),
                "{} に title が残る",
                route.attr.name
            );
            // $schema は実機検証まで残す (見直し条件で除去)。
            // properties 配下に null / 空配列 default が残らない。
            if let Some(serde_json::Value::Object(props)) = schema.get("properties") {
                for (field_name, prop) in props {
                    if let serde_json::Value::Object(field) = prop {
                        match field.get("default") {
                            Some(serde_json::Value::Null) => {
                                panic!("{}.{field_name} に null default が残る", route.attr.name)
                            }
                            Some(serde_json::Value::Array(a)) if a.is_empty() => {
                                panic!("{}.{field_name} に空配列 default が残る", route.attr.name)
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn kickoff_context_surfaces_setup_signals_without_raw_dump() {
        let repo = temp_git_repo("owox-kickoff-context");
        let owox = repo.join(".owox");
        std::fs::create_dir_all(repo.join("src")).unwrap();
        std::fs::create_dir_all(repo.join("domain")).unwrap();
        std::fs::create_dir_all(repo.join("infra")).unwrap();
        std::fs::create_dir_all(&owox).unwrap();
        std::fs::write(
            repo.join("Cargo.toml"),
            "[package]\nname = \"x\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        std::fs::write(repo.join("src/main.rs"), "fn main() {}\n").unwrap();
        std::fs::write(repo.join("domain/core.rs"), "pub fn core() {}\n").unwrap();
        std::fs::write(repo.join("infra/db.rs"), "pub fn db() {}\n").unwrap();

        let out = render_kickoff_context(&repo, &owox, &owox_core::Canon::default(), &[], &[]);
        assert!(out.contains("# Kickoff context"));
        assert!(out.contains("project nature draft"));
        assert!(out.contains("guardrail draft"));
        assert!(out.contains("verify entry draft: cargo test, cargo clippy"));
        assert!(out.contains("初期ガードレール案を採るか決める"));
        assert!(!out.contains("quality_toml"));
        assert!(!out.contains("rules_markdown"));
    }

    fn axes_with(prioritization: owox_core::Prioritization) -> owox_core::Axes {
        owox_core::Axes {
            prioritization,
            ..owox_core::Axes::default()
        }
    }

    fn axes_with_shape(requirements_shape: owox_core::RequirementsShape) -> owox_core::Axes {
        owox_core::Axes {
            requirements_shape,
            ..owox_core::Axes::default()
        }
    }

    #[test]
    fn prfaq_benefit_missing_only_under_prfaq_without_benefit() {
        use owox_core::RequirementsShape::{Lightweight, Prfaq};
        // prfaq + 便益なし → 弾く。
        assert!(prfaq_benefit_missing(Some(&axes_with_shape(Prfaq)), false));
        // prfaq でも便益ありなら通す。
        assert!(!prfaq_benefit_missing(Some(&axes_with_shape(Prfaq)), true));
        // lightweight (prfaq でない) なら便益なしでも通す。
        assert!(!prfaq_benefit_missing(
            Some(&axes_with_shape(Lightweight)),
            false
        ));
        // profile 未解決は素通り。
        assert!(!prfaq_benefit_missing(None, false));
    }

    #[test]
    fn ai_priority_blocked_only_under_ideal_first_with_priority() {
        use owox_core::Prioritization::{IdealFirst, Incremental};
        // 理想先行 + 優先度設定 → 弾く。
        assert!(ai_priority_blocked(Some(&axes_with(IdealFirst)), true));
        // 理想先行でも優先度未設定なら通す。
        assert!(!ai_priority_blocked(Some(&axes_with(IdealFirst)), false));
        // 漸進 (理想先行でない) なら優先度を付けても通す。
        assert!(!ai_priority_blocked(Some(&axes_with(Incremental)), true));
        // profile 未解決は素通り。
        assert!(!ai_priority_blocked(None, true));
    }

    fn finding(kind: &'static str, subject: &str) -> DecayFinding {
        DecayFinding {
            kind,
            subject: subject.to_string(),
            detail: "d".to_string(),
        }
    }

    #[test]
    fn next_clean_when_nothing_open_or_decaying() {
        let out = render_next(
            &[],
            &[],
            &[],
            &[],
            &[],
            &[],
            &[],
            owox_core::Axes::default(),
            AutoApproval {
                profile: false,
                session: false,
            },
            crate::cache::Mission::Work,
        );
        assert!(out.contains("nothing is decaying"));
    }

    #[test]
    fn next_announces_open_auto_window() {
        // session 窓 (auto_session=true) の文言。
        let out = render_next(
            &[],
            &[],
            &[],
            &[],
            &[],
            &[],
            &[],
            owox_core::Axes::default(),
            AutoApproval {
                profile: false,
                session: true,
            },
            crate::cache::Mission::Work,
        );
        assert!(out.contains("Automatic approval is on"));
        assert!(out.contains("this session"));
        assert!(out.contains("gate.auto_approve"));
    }

    #[test]
    fn next_announces_profile_auto_as_permanent() {
        // profile 由来 (flat) の auto は永続な文言で知らせる。
        let flat = owox_core::Axes {
            architecture: owox_core::Architecture::Flat,
            ..owox_core::Axes::default()
        };
        let out = render_next(
            &[],
            &[],
            &[],
            &[],
            &[],
            &[],
            &[],
            flat,
            AutoApproval {
                profile: true,
                session: false,
            },
            crate::cache::Mission::Work,
        );
        assert!(out.contains("Automatic approval is on"));
        assert!(out.contains("nature"));
        assert!(out.contains("gate.auto_approve"));
    }

    #[test]
    fn next_lists_auto_pending_for_confirmation() {
        let d = owox_core::Decision {
            id: "20260619-x".to_string(),
            title: "Proposed practice".to_string(),
            status: DecisionStatus::Adopted,
            rationale: "from a correction".to_string(),
            links: owox_core::DecisionLinks::default(),
            supersedes: Vec::new(),
            proposed_change: None,
            authorizes: Vec::new(),
            consumed: false,
            approval: Some("Auto-approved by owox. — 20260619".to_string()),
            auto_approved: true,
            confirmed: false,
        };
        let out = render_next(
            std::slice::from_ref(&d),
            &[],
            &[],
            &[],
            &[],
            &[],
            &[],
            owox_core::Axes::default(),
            AutoApproval {
                profile: false,
                session: false,
            },
            crate::cache::Mission::Work,
        );
        assert!(out.contains("Auto-approved, awaiting the human's confirmation"));
        assert!(out.contains("gate.confirm"));
        assert!(out.contains("20260619-x"));
    }

    #[test]
    fn open_decisions_tag_gate_autonomy() {
        let practice = owox_core::Decision {
            id: "20260619-p".to_string(),
            title: "Proposed practice from correction".to_string(),
            status: DecisionStatus::Open,
            rationale: "from a correction".to_string(),
            links: owox_core::DecisionLinks::default(),
            supersedes: Vec::new(),
            proposed_change: Some(owox_core::ProposedChange {
                target: "practices".to_string(),
                heading: "Practices".to_string(),
                op: "add".to_string(),
                item: "use Japanese for user-facing text".to_string(),
                to: None,
            }),
            authorizes: Vec::new(),
            consumed: false,
            approval: None,
            auto_approved: false,
            confirmed: false,
        };
        let plain = owox_core::Decision {
            id: "20260619-q".to_string(),
            title: "Plain open decision".to_string(),
            status: DecisionStatus::Open,
            rationale: String::new(),
            links: owox_core::DecisionLinks::default(),
            supersedes: Vec::new(),
            proposed_change: None,
            authorizes: Vec::new(),
            consumed: false,
            approval: None,
            auto_approved: false,
            confirmed: false,
        };
        let decisions = vec![practice, plain];
        let out = render_next(
            &decisions,
            &[],
            &[],
            &[],
            &[],
            &[],
            &[],
            owox_core::Axes::default(),
            AutoApproval {
                profile: false,
                session: true,
            },
            crate::cache::Mission::Work,
        );
        // practice 草案は非 guarded として auto 承認可を示す。
        assert!(out.contains("20260619-p) [non-guarded:"));
        assert!(out.contains("gate.auto_approve"));
        // 素の open decision は guarded として人間のみを示す。
        assert!(out.contains("20260619-q) [guarded:"));
    }

    fn ready_task_with_stage(stage: &str) -> Task {
        Task {
            id: "20260620-t".to_string(),
            title: "Wire the port".to_string(),
            status: owox_core::TaskStatus::Todo,
            links: TaskLinks::default(),
            deps: Vec::new(),
            notes: Vec::new(),
            layer: None,
            stage: Some(stage.to_string()),
            external: Vec::new(),
        }
    }

    fn accepted_req(
        id: &str,
        layer: Option<&str>,
        priority: Option<u32>,
    ) -> owox_core::Requirement {
        owox_core::Requirement {
            id: id.to_string(),
            title: format!("req {id}"),
            status: RequirementStatus::Accepted,
            statement: "s".to_string(),
            criteria: Vec::new(),
            links: owox_core::RequirementLinks::default(),
            supersedes: Vec::new(),
            priority,
            layer: layer.map(str::to_string),
            stage: None,
            kind: None,
        }
    }

    #[test]
    fn phased_axis_tags_ready_task_stage() {
        let task = ready_task_with_stage("mvp");
        // delivery=phased の時だけ stage を添える。
        let phased = owox_core::Axes::default();
        assert!(phased.phased_active());
        let out = render_next(
            &[],
            std::slice::from_ref(&task),
            &[],
            &[],
            &[],
            &[],
            &[],
            phased,
            AutoApproval {
                profile: false,
                session: false,
            },
            crate::cache::Mission::Work,
        );
        assert!(out.contains("[stage: mvp]"));
        // delivery=continuous なら stage を伏せる (段階化しない性質)。
        let continuous = owox_core::Axes {
            delivery: owox_core::Delivery::Continuous,
            ..owox_core::Axes::default()
        };
        let out = render_next(
            &[],
            &[task],
            &[],
            &[],
            &[],
            &[],
            &[],
            continuous,
            AutoApproval {
                profile: false,
                session: false,
            },
            crate::cache::Mission::Work,
        );
        assert!(!out.contains("[stage:"));
    }

    #[test]
    fn layered_axis_reports_layer_progress() {
        // 1 層に accepted だが基準未連結 (traced でない) 要件 → 0/1。
        let reqs = vec![accepted_req("20260620-a", Some("core"), None)];
        let layered = owox_core::Axes::default();
        assert!(layered.layered_active());
        let out = render_next(
            &[],
            &[],
            &reqs,
            &[],
            &[],
            &[],
            &[],
            layered,
            AutoApproval {
                profile: false,
                session: false,
            },
            crate::cache::Mission::Work,
        );
        assert!(out.contains("## Layer progress"));
        assert!(out.contains("core: 0/1 requirements traced"));
        // architecture=flat なら層別進行度を伏せる (層機構を持たない性質)。
        let flat = owox_core::Axes {
            architecture: owox_core::Architecture::Flat,
            ..owox_core::Axes::default()
        };
        let out = render_next(
            &[],
            &[],
            &reqs,
            &[],
            &[],
            &[],
            &[],
            flat,
            AutoApproval {
                profile: false,
                session: false,
            },
            crate::cache::Mission::Work,
        );
        assert!(!out.contains("## Layer progress"));
    }

    #[test]
    fn ideal_first_axis_lists_unprioritized() {
        // 優先度未設定の accepted 要件 → 並べ替え促し。
        let reqs = vec![accepted_req("20260620-b", None, None)];
        let ideal = owox_core::Axes::default();
        assert!(ideal.ideal_first_active());
        let out = render_next(
            &[],
            &[],
            &reqs,
            &[],
            &[],
            &[],
            &[],
            ideal,
            AutoApproval {
                profile: false,
                session: false,
            },
            crate::cache::Mission::Work,
        );
        assert!(out.contains("## Requirements to prioritize"));
        assert!(out.contains("requirement.update"));
        // prioritization=incremental なら並べ替え促しを伏せる (理想先行しない性質)。
        let incremental = owox_core::Axes {
            prioritization: owox_core::Prioritization::Incremental,
            ..owox_core::Axes::default()
        };
        let out = render_next(
            &[],
            &[],
            &reqs,
            &[],
            &[],
            &[],
            &[],
            incremental,
            AutoApproval {
                profile: false,
                session: false,
            },
            crate::cache::Mission::Work,
        );
        assert!(!out.contains("## Requirements to prioritize"));
    }

    #[test]
    fn decay_section_summarizes_by_kind() {
        let decay = vec![
            finding("stale", "20260101-a"),
            finding("stale", "20260101-b"),
            finding("zombie", "20260101-c"),
        ];
        let out = render_next(
            &[],
            &[],
            &[],
            &decay,
            &[],
            &[],
            &[],
            owox_core::Axes::default(),
            AutoApproval {
                profile: false,
                session: false,
            },
            crate::cache::Mission::Work,
        );
        assert!(out.contains("## Decay warnings"));
        assert!(out.contains("3 finding(s)"));
        assert!(out.contains("stale: 2"));
        assert!(out.contains("zombie: 1"));
    }

    #[test]
    fn decay_section_caps_item_list() {
        // 上限を超えたら個別項目は省略し残り件数を示す (context 汚染対策)。
        let decay: Vec<_> = (0..12)
            .map(|i| finding("stale", &format!("id-{i}")))
            .collect();
        let out = render_next(
            &[],
            &[],
            &[],
            &decay,
            &[],
            &[],
            &[],
            owox_core::Axes::default(),
            AutoApproval {
                profile: false,
                session: false,
            },
            crate::cache::Mission::Work,
        );
        assert!(out.contains("and 4 more"));
    }

    #[test]
    fn routine_section_lists_suggestions() {
        let routines = vec![owox_core::RoutineSuggestion {
            sequence: vec!["task.create".to_string(), "task.note".to_string()],
            occurrences: 6,
            kind: owox_core::RoutineKind::Skill,
            reasons: vec!["repeated 6 times".to_string()],
            suggested_script: None,
            test_hint: None,
        }];
        let out = render_next(
            &[],
            &[],
            &[],
            &[],
            &routines,
            &[],
            &[],
            owox_core::Axes::default(),
            AutoApproval {
                profile: false,
                session: false,
            },
            crate::cache::Mission::Work,
        );
        assert!(out.contains("Routines you could grow into a skill"));
        assert!(out.contains("task.create → task.note"));
        assert!(out.contains("seen 6x"));
    }

    #[test]
    fn gardening_section_lists_candidates() {
        let gardening = vec![GardeningFinding {
            kind: "duplicate-practice".to_string(),
            severity: "advisory",
            subject: "20260620-practice".to_string(),
            detail: "looks duplicated".to_string(),
        }];
        let out = render_next(
            &[],
            &[],
            &[],
            &[],
            &[],
            &gardening,
            &[],
            owox_core::Axes::default(),
            AutoApproval {
                profile: false,
                session: false,
            },
            crate::cache::Mission::Work,
        );
        assert!(out.contains("## Gardening candidates"));
        assert!(out.contains("duplicate-practice"));
    }

    #[test]
    fn kickoff_next_renders_one_question() {
        let questions = vec![KickoffQuestion {
            stage: "安全境界",
            item: "初期ガードレール案を採るか決める".to_string(),
            recommendation: "検出案を初期値として採る".to_string(),
            reason: "既存コードから案が取れている".to_string(),
            decider: "human",
            options: vec!["検出案を採る".to_string(), "手で決める".to_string()],
        }];
        let out = render_kickoff_next(&questions);
        assert!(out.contains("Stage: 安全境界"));
        assert!(out.contains("Recommended: 検出案を初期値として採る"));
        assert!(out.contains("Decider: human"));
    }

    #[test]
    fn kickoff_question_json_has_required_fields() {
        let value = kickoff_question_json(&KickoffQuestion {
            stage: "初期 task",
            item: "最初の task 分割".to_string(),
            recommendation: "AI仮決定".to_string(),
            reason: "repo 構造から切りやすい".to_string(),
            decider: "ai",
            options: vec!["AI仮決定".to_string()],
        });
        assert_eq!(value["decider"], "ai");
        assert_eq!(value["stage"], "初期 task");
        assert!(value["options"].is_array());
    }

    #[test]
    fn kickoff_status_lists_return_candidates() {
        let repo = temp_git_repo("owox-kickoff-status");
        let owox = repo.join(".owox");
        std::fs::create_dir_all(repo.join("src")).unwrap();
        std::fs::create_dir_all(&owox).unwrap();
        std::fs::write(
            repo.join("Cargo.toml"),
            "[package]\nname = \"x\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        std::fs::write(repo.join("src/main.rs"), "fn main() {}\n").unwrap();
        let value = kickoff_status_json(&owox, &repo, &owox_core::Canon::default(), &[], &[]);
        assert_eq!(value["ready_to_return"], serde_json::json!(false));
        let candidates = value["canonicalization_candidates"].as_array().unwrap();
        assert!(candidates.iter().any(|c| c["route"] == "profile.set"));
        assert!(
            candidates
                .iter()
                .any(|c| c["route"] == "config.toml [[verify.checks]]")
        );
        assert!(candidates.iter().any(|c| c["route"] == "task.create"));
    }

    #[test]
    fn mission_preview_for_kickoff_returns_question_text() {
        let repo = temp_git_repo("owox-mission-preview");
        let owox = repo.join(".owox");
        std::fs::create_dir_all(repo.join("src")).unwrap();
        std::fs::create_dir_all(&owox).unwrap();
        std::fs::write(
            repo.join("Cargo.toml"),
            "[package]\nname = \"x\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        std::fs::write(repo.join("src/main.rs"), "fn main() {}\n").unwrap();
        let preview = mission_preview(&owox, &repo, crate::cache::Mission::Kickoff).unwrap();
        assert!(preview.contains("What to decide next"));
        assert!(preview.contains("Recommended:"));
    }

    #[test]
    fn floor_bloat_detected_when_floor_is_too_large() {
        let mut canon = owox_core::Canon::default();
        canon.brand.vision = "x".repeat(20_000);
        let findings = floor_bloat_findings(&canon);
        assert!(findings.iter().any(|finding| finding.kind == "floor-bloat"));
    }

    #[test]
    fn command_routing_detects_missing_required_tool_mentions() {
        let dir = std::env::temp_dir().join(format!("owox-routing-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("commands.toml"),
            "[[command]]\nname = \"review\"\ndescription = \"r\"\nbody = \"Only call verify.run\"\n",
        )
        .unwrap();
        let findings = command_routing_findings(&dir);
        assert!(
            findings
                .iter()
                .any(|finding| { finding.kind == "entry-routing" && finding.subject == "review" })
        );
    }

    #[test]
    fn generated_drift_detects_modified_generated_file() {
        let repo =
            std::env::temp_dir().join(format!("owox-generated-drift-{}", std::process::id()));
        let owox = repo.join(".owox");
        let _ = std::fs::remove_dir_all(&repo);
        std::fs::create_dir_all(&owox).unwrap();
        std::fs::create_dir_all(repo.join(".codex")).unwrap();
        std::fs::write(repo.join(".codex/hooks.json"), "{\"broken\":true}\n").unwrap();
        let findings = generated_drift_findings(&owox, &repo, &owox_core::Canon::default());
        assert!(
            findings
                .iter()
                .any(|finding| finding.kind == "generated-drift")
        );
    }

    #[test]
    fn low_use_skill_detected_after_age_and_repo_activity() {
        let repo = temp_git_repo("owox-low-use-skill");
        let owox = repo.join(".owox");
        git_commit(
            &repo,
            ".owox/skills/quiet/SKILL.md",
            "---\nname: quiet\ndescription: q\n---\n\nDo the quiet task.\n",
            "2026-01-01T00:00:00Z",
            "add skill",
        );
        for i in 0..20 {
            git_commit(
                &repo,
                "notes.txt",
                &format!("note {i}\n"),
                "2026-03-01T00:00:00Z",
                &format!("note-{i}"),
            );
        }
        let findings = low_use_skill_findings(&owox, &repo, "20260626");
        assert!(
            findings
                .iter()
                .any(|finding| finding.kind == "low-use" && finding.subject == "skill quiet")
        );
    }

    #[test]
    fn low_use_practice_detected_only_outside_floor_budget() {
        let repo = temp_git_repo("owox-low-use-practice");
        git_commit(
            &repo,
            "notes.txt",
            "start\n",
            "2026-03-01T00:00:00Z",
            "start",
        );
        for i in 0..20 {
            git_commit(
                &repo,
                "notes.txt",
                &format!("note {i}\n"),
                "2026-04-01T00:00:00Z",
                &format!("note-{i}"),
            );
        }
        let mut canon = owox_core::Canon::default();
        canon.settings.context.practices_floor_max = 1;
        canon.practices.entries = vec![
            owox_core::Practice {
                date: "20260601".to_string(),
                text: "fresh".to_string(),
            },
            owox_core::Practice {
                date: "20260301".to_string(),
                text: "older".to_string(),
            },
        ];
        let findings = low_use_practice_findings(&repo, &canon, "20260626");
        assert!(
            findings
                .iter()
                .any(|finding| finding.kind == "low-use" && finding.subject == "practice 20260301")
        );
        assert!(
            findings
                .iter()
                .all(|finding| finding.subject != "practice 20260601")
        );
    }

    #[test]
    fn next_mentions_kickoff_mission() {
        let out = render_next(
            &[],
            &[],
            &[],
            &[],
            &[],
            &[],
            &[],
            owox_core::Axes::default(),
            AutoApproval {
                profile: false,
                session: false,
            },
            crate::cache::Mission::Kickoff,
        );
        assert!(out.contains("Kickoff mission is active"));
    }
}
