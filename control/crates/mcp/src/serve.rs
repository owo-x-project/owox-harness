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
        let work_dir = self.repo_root();
        let files = crate::files::list_repo_files(work_dir);
        let has_quality_layers = owox_core::load_canon(&self.owox_dir)
            .map(|c| !c.quality.layers.is_empty() || !c.quality.boundaries.is_empty())
            .unwrap_or(false);
        let has_version_tags = git_has_version_tags(work_dir);
        (files, has_quality_layers, has_version_tags)
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

#[tool_router(router = tool_router)]
impl OwoxServer {
    /// 判断を来歴へ記録する。status=open は未決の人間ゲートになる。
    #[tool(
        name = "decision.record",
        description = "Record a durable design or direction decision in the log under .owox/decisions/. Only for decisions future work must not silently reverse, not transient working state — use task.note for memos. Use status=open when it still needs human judgment (a pending gate); adopted/rejected/superseded for a settled record."
    )]
    async fn decision_record(
        &self,
        Parameters(p): Parameters<DecisionRecordParams>,
    ) -> Result<CallToolResult, McpError> {
        let status = match DecisionStatus::parse(&p.status) {
            Ok(s) => s,
            Err(err) => return envelope_result(Envelope::failed(err)),
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
            envelope_result(record_decision(&self.owox_dir, &today_utc(), input))
        } else {
            envelope_result(owox_core::record_decision_with_authorization(
                &self.owox_dir,
                &today_utc(),
                input,
                p.authorizes,
            ))
        }
    }

    /// 未承認の判断点 (status=open の来歴) を一覧する。
    #[tool(
        name = "gate.list",
        description = "List pending human gates: decisions with status=open still needing human judgment."
    )]
    async fn gate_list(&self) -> Result<CallToolResult, McpError> {
        envelope_result(list_gates(&self.owox_dir))
    }

    /// 文脈ナビ。作業 → 読む先の地図を返す。canon を直読みせずここから取る。
    ///
    /// 読みは tool に一本化した (resource はモデルが取りに来ず不安定。
    /// `docs/decisions/20260613-Phase5-実機検証の是正.md`)。封筒でなく描画した本文を返す。
    #[tool(
        name = "context",
        description = "Get the context map: for the current task or path, which files to read and which rules apply. Call this instead of reading .owox/ directly."
    )]
    async fn context(&self) -> Result<CallToolResult, McpError> {
        let canon = owox_core::load_canon(&self.owox_dir)
            .map_err(|err| McpError::internal_error(format!("正本を読めない: {err}"), None))?;
        Ok(CallToolResult::success(vec![Content::text(
            render_context(&canon.context),
        )]))
    }

    /// 次の一手。未決の人間ゲート (status=open の来歴) と ready タスクを返す。
    #[tool(
        name = "next",
        description = "Get what to act on next: open decisions awaiting human judgment and tasks ready to start."
    )]
    async fn next(&self) -> Result<CallToolResult, McpError> {
        let decisions = list_decisions(&self.owox_dir)
            .map_err(|err| McpError::internal_error(format!("来歴を読めない: {err}"), None))?;
        let tasks = list_tasks(&self.owox_dir)
            .map_err(|err| McpError::internal_error(format!("タスクを読めない: {err}"), None))?;
        let requirements = list_requirements(&self.owox_dir)
            .map_err(|err| McpError::internal_error(format!("要件を読めない: {err}"), None))?;
        // 腐敗検知の閾値は quality.toml の [decay]。正本が読めない時は警告を出さない (作業を妨げない)。
        // 成長層 (practices) の鮮度も合流する (canon 内で軽い・next の高速性を崩さない)。
        let decay = owox_core::load_canon(&self.owox_dir)
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
        let routines = owox_core::load_canon(&self.owox_dir)
            .map(|canon| {
                owox_core::run_routine_suggestions(&self.owox_dir, &canon.quality.routine, &skills)
            })
            .unwrap_or_default();
        // 性質軸を解決する (profile.toml)。読めない/解決失敗時はフル方法論既定で振る舞う。
        let axes = self.resolved_axes();
        // 自動承認の同意源2系統 (profile 由来=永続・session 由来=session 限り)。
        let auto = self.auto_sources();
        Ok(CallToolResult::success(vec![Content::text(render_next(
            &decisions,
            &tasks,
            &requirements,
            &decay,
            &routines,
            axes,
            auto,
        ))]))
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
        description = "Approve a pending gate: transition an open decision to adopted and record the approval. Call this yourself when the human decides to approve — they cannot call it. Calling it shows a CLI confirmation prompt, which is the human's approval gate; do not ask them to run a tool or act first. If the gate carries a proposed canon change from canon.propose op=remove/replace, approving applies it."
    )]
    async fn gate_approve(
        &self,
        Parameters(p): Parameters<GateApproveParams>,
    ) -> Result<CallToolResult, McpError> {
        // canon 変更が紐づくなら、承認 = canon へ適用。適用に失敗したら承認しない。
        if let Err(err) = owox_core::apply_pending_canon_change(&self.owox_dir, &p.id) {
            return envelope_result(Envelope::failed(err));
        }
        envelope_result(approve_gate(
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
        description = "Turn on automatic approval for this session so you can approve non-guarded gates yourself with gate.auto_approve while the human is away. Only needed for cautious projects; a flat-architecture project already has automatic approval on by its nature. Call only when the human tells you to proceed automatically — its CLI confirmation prompt is their consent. Lasts this session and closes at the next session start; turn it off sooner with gate.auto_disable. Guarded gates (changes to brand, rules, glossary, and plain open decisions) still need gate.approve."
    )]
    async fn gate_auto_enable(&self) -> Result<CallToolResult, McpError> {
        crate::cache::open_auto_window(&self.owox_dir);
        envelope_result(Envelope::ok(
            "Automatic approval is on for this session. Approve non-guarded gates with gate.auto_approve; they are queued for the human to confirm or revert. It closes at the next session start.",
            serde_json::json!({ "auto_window": true }),
        ))
    }

    /// 自動承認の session 窓を閉じる。profile 由来 (flat) の auto は残る (profile.set で性質を変えるまで)。
    #[tool(
        name = "gate.auto_disable",
        description = "Close the session's automatic-approval window. On a cautious project this turns automatic approval off, so non-guarded gates again need gate.approve. On a flat-architecture project automatic approval stays on by its nature; change that with profile.set, not here."
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
        envelope_result(Envelope::ok(
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
        description = "Approve a non-guarded gate automatically, without a human confirmation prompt, while automatic approval is on. Automatic approval is on either by project nature (a flat-architecture project keeps it on) or for the session after gate.auto_enable. Refuses guarded gates (changes to brand, rules, glossary, and plain open decisions) and refuses when automatic approval is off; use gate.approve for those. owox marks what it auto-approves and lists it in the next tool for the human to confirm with gate.confirm or undo with gate.revert."
    )]
    async fn gate_auto_approve(
        &self,
        Parameters(p): Parameters<GateIdParams>,
    ) -> Result<CallToolResult, McpError> {
        if !self.auto_sources().active() {
            return envelope_result(Envelope::failed(
                "Automatic approval is off. Ask the human to approve with gate.approve, or have them turn on automatic approval with gate.auto_enable first.",
            ));
        }
        // guarded は auto 不可。適用前に判定し、固定層 canon を無確認で変えないようにする。
        let decision = match owox_core::load_decision(&self.owox_dir, &p.id) {
            Ok(d) => d,
            Err(err) => return envelope_result(Envelope::failed(err)),
        };
        if owox_core::gate_autonomy(&decision) == owox_core::Autonomy::Guarded {
            return envelope_result(Envelope::failed(format!(
                "Gate {} is guarded and only a human can approve it. Use gate.approve.",
                p.id
            )));
        }
        // 紐づく canon 変更があれば適用する (gate.approve と同じ合成)。失敗したら承認しない。
        if let Err(err) = owox_core::apply_pending_canon_change(&self.owox_dir, &p.id) {
            return envelope_result(Envelope::failed(err));
        }
        envelope_result(approve_gate_auto(&self.owox_dir, &today_utc(), &p.id))
    }

    /// 自動承認を人間が後から確認済みにする。後追いキューから外れる。
    #[tool(
        name = "gate.confirm",
        description = "Confirm an auto-approved decision after the human has reviewed it, removing it from the follow-up queue in the next tool. The id comes from that queue."
    )]
    async fn gate_confirm(
        &self,
        Parameters(p): Parameters<GateIdParams>,
    ) -> Result<CallToolResult, McpError> {
        envelope_result(owox_core::confirm_auto_approval(
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
        description = "Undo an auto-approved decision: revert any canon change it applied and mark it rejected. Use when the human reviews the follow-up queue and rejects one. The id comes from the next tool's auto-approved list."
    )]
    async fn gate_revert(
        &self,
        Parameters(p): Parameters<GateIdParams>,
    ) -> Result<CallToolResult, McpError> {
        // canon を元へ戻す。失敗したら差し戻さない (来歴と canon の食い違いを作らない)。
        if let Err(err) = owox_core::revert_pending_canon_change(&self.owox_dir, &p.id) {
            return envelope_result(Envelope::failed(err));
        }
        envelope_result(owox_core::reject_decision(
            &self.owox_dir,
            &today_utc(),
            &p.id,
            None,
        ))
    }

    /// 人間の訂正から practice 草案を起草する。固定はせず open gate として積む。
    #[tool(
        name = "correction.note",
        description = "When the human corrects or overrides something you did, capture the durable lesson here instead of waiting to be told to make a rule. owox drafts it as a proposed practice recorded as an open gate; it is not fixed until a human approves the gate (gate.approve, or gate.auto_approve while automatic approval is on), after which owox adds it to the practices."
    )]
    async fn correction_note(
        &self,
        Parameters(p): Parameters<CorrectionNoteParams>,
    ) -> Result<CallToolResult, McpError> {
        envelope_result(owox_core::propose_practice_from_correction(
            &self.owox_dir,
            &today_utc(),
            p.summary.as_deref().unwrap_or(""),
            &p.lesson,
        ))
    }

    /// 要件を作る。受け入れ基準をまとめて受けられる。
    #[tool(
        name = "requirement.create",
        description = "Create a NEW requirement in .owox/requirements/. It has a status (draft default; accepted/superseded human-declared), a statement of what must hold, acceptance criteria (each given/when/then with an optional verification link), and a kind (functional or non-functional). Keep technical or design constraints as decisions via decision.record, not requirements. Working backwards: pass benefit (who gains and why), recorded as a linked decision and required under prfaq requirements-shape. Under ideal-first prioritization do not set priority; propose a ranking, let a human decide, then record it with requirement.update. To re-scope an existing one use requirement.update with a reason, not a duplicate."
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
            Err(err) => return envelope_result(Envelope::failed(err)),
        };
        let kind = match p.kind.as_deref().map(RequirementKind::parse).transpose() {
            Ok(k) => k,
            Err(err) => return envelope_result(Envelope::failed(err)),
        };
        // 理想先行では優先度の並び替えは人間の判断。AI が起草時にランクを付けるのを弾く
        // (`docs/decisions/20260620-要件分類とPRFAQ正本.md`)。profile が読めない時は素通り (安全側)。
        let axes = owox_core::load_canon(&self.owox_dir)
            .ok()
            .and_then(|c| c.profile.resolve().ok());
        if ai_priority_blocked(axes.as_ref(), p.priority.is_some()) {
            return envelope_result(Envelope::failed(
                "Under ideal-first prioritization, the priority ranking is a human decision. Create the requirement without priority, propose a ranking to the human, and set it with requirement.update only after they decide.",
            ));
        }
        let benefit_set = p.benefit.as_deref().is_some_and(|b| !b.trim().is_empty());
        if prfaq_benefit_missing(axes.as_ref(), benefit_set) {
            return envelope_result(Envelope::failed(
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
        envelope_result(create_requirement(
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
        description = "List requirements with status, acceptance criteria count, and how many criteria still lack a verification link. Optionally filter by status."
    )]
    async fn requirement_list(
        &self,
        Parameters(p): Parameters<RequirementListParams>,
    ) -> Result<CallToolResult, McpError> {
        envelope_result(list_requirements_envelope(
            &self.owox_dir,
            p.status.as_deref(),
        ))
    }

    /// 要件 1 件を全文読む。canon を直読みせずここから取る。
    #[tool(
        name = "requirement.get",
        description = "Get one requirement in full: statement, acceptance criteria with given/when/then and verification links, status, and links. Use instead of reading .owox/ directly."
    )]
    async fn requirement_get(
        &self,
        Parameters(p): Parameters<RequirementGetParams>,
    ) -> Result<CallToolResult, McpError> {
        envelope_result(get_requirement(&self.owox_dir, &p.id))
    }

    /// 要件の title・statement・状態を変える。本質変更は reason 必須・来歴連動。
    #[tool(
        name = "requirement.update",
        description = "Update a requirement's status, title, or statement. Changing the title or statement is a content change requiring a reason, recorded as a linked decision; status changes are lightweight. To add or link acceptance criteria use requirement.add_criterion and requirement.link_verification."
    )]
    async fn requirement_update(
        &self,
        Parameters(p): Parameters<RequirementUpdateParams>,
    ) -> Result<CallToolResult, McpError> {
        envelope_result(update_requirement(
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
        description = "Add an acceptance criterion (given/when/then) to a requirement, assigned the next criterion number. Link a verification to it later with requirement.link_verification."
    )]
    async fn requirement_add_criterion(
        &self,
        Parameters(p): Parameters<RequirementAddCriterionParams>,
    ) -> Result<CallToolResult, McpError> {
        envelope_result(add_criterion(
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
        description = "Link a verification to an acceptance criterion of a requirement. It must be a check name declared in [[verify.checks]] in config.toml; an unknown name is rejected with the available names listed. This is the requirement-to-test trace; verify.run later uses it to machine-judge requirement completion."
    )]
    async fn requirement_link_verification(
        &self,
        Parameters(p): Parameters<RequirementLinkVerificationParams>,
    ) -> Result<CallToolResult, McpError> {
        envelope_result(link_verification(
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
        description = "Select which review perspectives apply to the current change (correctness, design, security, plan-alignment, requirement, pruning, and any project-declared lenses), based on the files it touches. When reviewing: take in verify.run first, then review through each returned perspective, confirming and adversarially re-checking each finding. Treat pruning as a proposal routed through the deletion policy and verification, never a blind delete."
    )]
    async fn review_lenses(&self) -> Result<CallToolResult, McpError> {
        let work_dir = self.owox_dir.parent().unwrap_or(&self.owox_dir);
        let changed = crate::files::changed_files(work_dir);
        envelope_result(review_lenses_envelope(&self.owox_dir, &changed))
    }

    /// 完了を3区別して返す。検証完了だけ機械判定、作業・要件完了は人間判断。
    #[tool(
        name = "verify.run",
        description = "Run the project's configured verification checks and report completion in three kinds: work, requirement, verification. Verification is machine-judged from [[verify.checks]] in config.toml; work and requirement completion return needs_human."
    )]
    async fn verify_run(&self) -> Result<CallToolResult, McpError> {
        let canon = match owox_core::load_canon(&self.owox_dir) {
            Ok(canon) => canon,
            Err(err) => {
                return envelope_result(Envelope::failed(format!("正本を読めない: {err}")));
            }
        };
        // 検査は target repo ルート (`.owox` の親) で実行する。
        let work_dir = self.owox_dir.parent().unwrap_or(&self.owox_dir);
        let requirements = match list_requirements(&self.owox_dir) {
            Ok(r) => r,
            Err(err) => {
                return envelope_result(Envelope::failed(format!("要件を読めない: {err}")));
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
        if !routines.is_empty() {
            let items: Vec<serde_json::Value> = routines
                .iter()
                .map(
                    |r| serde_json::json!({ "sequence": r.sequence, "occurrences": r.occurrences }),
                )
                .collect();
            merge_data(&mut env, "routines", serde_json::Value::Array(items));
        }
        envelope_result(env)
    }

    /// 配布前に成果物を検証する (配布運用がある対象プロジェクトだけ)。
    /// 版抽出・成果物存在・委譲検査を封筒で返す。owox 自身は hash を計算しない
    /// (`docs/decisions/20260621-Phase10-配布とrelease正本.md`)。
    #[tool(
        name = "release.check",
        description = "Verify a release before publishing, for projects that ship one: read .owox/release.toml, extract the current version from its configured file, confirm every expected artifact exists under the dist directory, and run the project's delegated artifact-verification checks (checksums, signing). Reports no-op guidance when no release.toml exists."
    )]
    async fn release_check(
        &self,
        Parameters(p): Parameters<ReleaseCheckParams>,
    ) -> Result<CallToolResult, McpError> {
        let canon = match owox_core::load_canon(&self.owox_dir) {
            Ok(canon) => canon,
            Err(err) => {
                return envelope_result(Envelope::failed(format!("正本を読めない: {err}")));
            }
        };
        let release = &canon.release;
        // 配布運用なし。release.toml を置く対象プロジェクトだけが使う。
        if release.policy.is_empty()
            && release.version.is_none()
            && release.artifacts.is_empty()
            && release.checks.is_empty()
        {
            return envelope_result(Envelope::ok(
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
            return envelope_result(
                Envelope::failed(format!("配布前検証が通らない: {}", reasons.join(" / ")))
                    .with_data(data),
            );
        }

        envelope_result(Envelope::ok("配布前検証が通った", data))
    }

    /// プロジェクト状態 (phase) を宣言する。機械ゲートの厳しさが変わる。
    #[tool(
        name = "state.set",
        description = "Declare the project phase: initial (early, lenient gates), stable, or maintenance (strict gates; open decisions block commit). Recorded in the decision log."
    )]
    async fn state_set(
        &self,
        Parameters(p): Parameters<StateSetParams>,
    ) -> Result<CallToolResult, McpError> {
        let phase = match Phase::parse(&p.phase) {
            Ok(phase) => phase,
            Err(err) => return envelope_result(Envelope::failed(err)),
        };
        envelope_result(set_state(&self.owox_dir, &today_utc(), phase))
    }

    /// プロジェクトの性質 (固定) を宣言する。開発方法論のモジュールが軸で出し入れされる。
    #[tool(
        name = "profile.set",
        description = "Declare the project's nature: pick a named bundle (clean-arch-app, script, library, data-platform, research) with preset, or override individual axes (requirements-shape, prioritization, delivery, architecture). Turns methodology modules on or off; recorded in the decision log. Nature is fixed but changeable later. For an existing project, run profile.detect first and confirm the draft with a human."
    )]
    async fn profile_set(
        &self,
        Parameters(p): Parameters<ProfileSetParams>,
    ) -> Result<CallToolResult, McpError> {
        let overrides = match parse_partial_axes(&p) {
            Ok(o) => o,
            Err(err) => return envelope_result(Envelope::failed(err)),
        };
        envelope_result(owox_core::set_profile(
            &self.owox_dir,
            &today_utc(),
            p.preset,
            overrides,
        ))
    }

    /// 性質を既存コードから推定する (逆生成)。draft + 根拠を返し、確定はしない (人間ゲート)。
    #[tool(
        name = "profile.detect",
        description = "Detect the project's likely nature from existing code (reverse-generation): returns a draft of the four axes (requirements-shape, prioritization, delivery, architecture) with evidence, and the closest named bundle. A proposal only — it does NOT set anything. Show a human, then confirm with profile.set."
    )]
    async fn profile_detect(&self) -> Result<CallToolResult, McpError> {
        let (files, has_quality_layers, has_version_tags) = self.detect_inputs();
        let draft = owox_core::detect_profile(&owox_core::DetectSignals {
            files: &files,
            has_quality_layers,
            has_version_tags,
        });
        envelope_result(Envelope::ok(
            "Detected a draft project nature. Confirm with a human, then set it with profile.set.",
            profile_draft_value(&draft),
        ))
    }

    /// 既存コードから rules / quality の初期案を逆生成する。draft + 根拠を返し、確定しない (人間ゲート)。
    #[tool(
        name = "canon.detect",
        description = "Reverse-generate draft guardrails from existing code (kickoff): infers quality layers (core vs edge directories with autonomy), a dependency-direction boundary, and irreversible operations (migrations / Terraform / Kubernetes) — each with evidence and a ready-to-paste TOML or markdown snippet. A proposal only — it writes NOTHING. Review with a human, then paste into quality.toml / rules.md or add via canon.add. Run profile.detect for the project's nature."
    )]
    async fn canon_detect(&self) -> Result<CallToolResult, McpError> {
        let (files, has_quality_layers, has_version_tags) = self.detect_inputs();
        let draft = owox_core::detect_canon_draft(&owox_core::DetectSignals {
            files: &files,
            has_quality_layers,
            has_version_tags,
        });
        if draft.is_empty() {
            return envelope_result(Envelope::ok(
                "No guardrails inferred from existing code (no layered directories or destructive-infra signals). Author rules / quality by hand if needed.",
                serde_json::json!({ "layers": [], "boundaries": [], "irreversible": [] }),
            ));
        }
        envelope_result(Envelope::ok(
            "Reverse-generated draft guardrails from existing code. Proposal only — review with a human, then paste the snippets into quality.toml / rules.md (or add via canon.add). Nothing was written.",
            canon_draft_value(&draft),
        ))
    }

    /// セッション立ち上げを束ねる。向き付け・性質・既存コードからの逆生成案を1呼び出しで返す。
    #[tool(
        name = "kickoff",
        description = "Orient at the start of a session in one call: returns the project's Vision, phase, and nature. If the nature is not declared yet, includes a reverse-generated draft nature (with evidence) to confirm with a human and set via profile.set. If you are adopting owox into an existing codebase with thin guardrails, also includes draft layers, boundaries, and irreversible operations to review with a human before adding via canon.add. Writes NOTHING. After this, call the next tool for open decisions and ready tasks and the context tool for what to read."
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
        envelope_result(Envelope::ok(
            "Oriented for this session. Stated below: Vision, phase, nature, and any reverse-generated drafts to confirm with a human. Nothing was written.",
            data,
        ))
    }

    /// 現在の性質 (解決済み実効軸) を返す。
    #[tool(
        name = "profile.get",
        description = "Get the project's current nature: the resolved four axes (requirements-shape, prioritization, delivery, architecture) and which methodology modules are active. Defaults to the full methodology when no profile is set."
    )]
    async fn profile_get(&self) -> Result<CallToolResult, McpError> {
        let profile = owox_core::load_canon(&self.owox_dir)
            .map(|c| c.profile)
            .unwrap_or_default();
        let axes = match profile.resolve() {
            Ok(a) => a,
            Err(err) => return envelope_result(Envelope::failed(err)),
        };
        envelope_result(Envelope::ok(
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
        description = "Append a working note to the current git branch's memory: what you are doing on this branch, an in-progress judgment, or a scratch thought. Keyed by branch, kept out of git, not injected into context — read on demand with branch.notes. Use for branch-scoped scratch; decision.record for durable judgments, task.note for task-scoped notes. Secrets are rejected."
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
        envelope_result(owox_core::add_branch_note(
            &work_root,
            &branch,
            &today_utc(),
            &p.text,
        ))
    }

    /// 現在のブランチの作業記憶を読む (オンデマンド)。
    #[tool(
        name = "branch.notes",
        description = "Read the current git branch's working memory: the notes recorded on this branch. On-demand; branch memory is never injected into the session context."
    )]
    async fn branch_notes(&self) -> Result<CallToolResult, McpError> {
        let work_dir = self.repo_root();
        let branch = git_current_branch(work_dir);
        let work_root = branch_work_root(work_dir, &self.owox_dir);
        envelope_result(owox_core::get_branch_memory_envelope(&work_root, &branch))
    }

    /// やることを 1 件作る。
    #[tool(
        name = "task.create",
        description = "Create a NEW task in .owox/tasks/. Tasks have a status (todo by default), links to a requirement/decision/verification, typed dependencies, and optional external refs mapping to an issue tracker (each \"system: reference\", e.g. \"github: owner/repo#123\"). To rename or re-scope an existing task use task.update with a reason, not a duplicate."
    )]
    async fn task_create(
        &self,
        Parameters(p): Parameters<TaskCreateParams>,
    ) -> Result<CallToolResult, McpError> {
        let deps = match parse_deps(p.deps) {
            Ok(d) => d,
            Err(err) => return envelope_result(Envelope::failed(err)),
        };
        let external = match parse_external(p.external) {
            Ok(e) => e,
            Err(err) => return envelope_result(Envelope::failed(err)),
        };
        let input = CreateTaskInput {
            title: p.title,
            links: p.links.into(),
            deps,
            layer: p.layer,
            stage: p.stage,
            external,
        };
        envelope_result(create_task(
            &self.owox_dir,
            &today_utc(),
            &self.known_layer_names(),
            input,
        ))
    }

    /// タスクを一覧する。ready=true で前提解決済のみ。
    #[tool(
        name = "task.list",
        description = "List tasks. Pass ready=true for only tasks whose blocking dependencies are all done (ready to work on). Optionally filter by status."
    )]
    async fn task_list(
        &self,
        Parameters(p): Parameters<TaskListParams>,
    ) -> Result<CallToolResult, McpError> {
        envelope_result(list_tasks_envelope(
            &self.owox_dir,
            p.ready,
            p.status.as_deref(),
        ))
    }

    /// タスクの title・状態・link・依存を変える (done は task.close)。
    #[tool(
        name = "task.update",
        description = "Update a task's title, status, links, add dependencies, or add external refs (each \"system: reference\", e.g. \"github: owner/repo#123\"; duplicates are not re-added). Changing the title is a content change requiring a reason, recorded as a decision; status/links/deps/external changes are lightweight. To mark a task done use task.close (it requires verification); task.update rejects the done transition."
    )]
    async fn task_update(
        &self,
        Parameters(p): Parameters<TaskUpdateParams>,
    ) -> Result<CallToolResult, McpError> {
        let add_deps = match parse_deps(p.deps) {
            Ok(d) => d,
            Err(err) => return envelope_result(Envelope::failed(err)),
        };
        let add_external = match parse_external(p.external) {
            Ok(e) => e,
            Err(err) => return envelope_result(Envelope::failed(err)),
        };
        envelope_result(update_task(
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
    #[tool(
        name = "task.note",
        description = "Append a short working note to a task. Use for transient working state (what you tried, what to check next) instead of decision.record, which is for durable design or direction decisions."
    )]
    async fn task_note(
        &self,
        Parameters(p): Parameters<TaskNoteParams>,
    ) -> Result<CallToolResult, McpError> {
        envelope_result(add_note(&self.owox_dir, &today_utc(), &p.id, &p.text))
    }

    /// タスクを依存でつなぐ。
    #[tool(
        name = "task.link",
        description = "Add a typed dependency to a task: blocks, parent-child, related, or discovered-from. A blocks dependency keeps the task out of the ready list until its target is done."
    )]
    async fn task_link(
        &self,
        Parameters(p): Parameters<TaskLinkParams>,
    ) -> Result<CallToolResult, McpError> {
        let dep = match p.dep.into_dep() {
            Ok(d) => d,
            Err(err) => return envelope_result(Envelope::failed(err)),
        };
        envelope_result(link_task(&self.owox_dir, &p.id, dep))
    }

    /// タスクを閉じる。検証を通らないと閉じれない (自己申告 done を排除)。
    #[tool(
        name = "task.close",
        description = "Close a task as done. Runs the configured verification checks first; the task only closes if they pass. With no checks configured it returns needs_human."
    )]
    async fn task_close(
        &self,
        Parameters(p): Parameters<TaskCloseParams>,
    ) -> Result<CallToolResult, McpError> {
        let canon = match owox_core::load_canon(&self.owox_dir) {
            Ok(canon) => canon,
            Err(err) => {
                return envelope_result(Envelope::failed(format!("正本を読めない: {err}")));
            }
        };
        let work_dir = self.owox_dir.parent().unwrap_or(&self.owox_dir);
        envelope_result(close_task(
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
        description = "Drop a task that is no longer needed. A reason is required and recorded in the decision log, so dropped work is never silently lost."
    )]
    async fn task_drop(
        &self,
        Parameters(p): Parameters<TaskDropParams>,
    ) -> Result<CallToolResult, McpError> {
        envelope_result(drop_task(&self.owox_dir, &today_utc(), &p.id, &p.reason))
    }

    /// スキルの 2 軸状態 (テスト・昇格) を一覧する。
    #[tool(
        name = "skill.list",
        description = "List the project's skills under .owox/skills/ with two axes: tests (passing/failing/none) and stage (draft/registered/promoted). Registered skills are generated and usable by name; promoted ones are human-approved and may auto-invoke. Draft skills are not generated; their problem field says why (a failing test, or a contract-lint violation such as a referenced scripts/<name> not bundled or a tests/ file not executable). Runs each skill's tests."
    )]
    async fn skill_list(&self) -> Result<CallToolResult, McpError> {
        envelope_result(list_skills_envelope(&self.owox_dir, self.repo_root()))
    }

    /// スキルのテストを実行し、合格・適格なら登録 (生成) する。
    #[tool(
        name = "skill.register",
        description = "Run a skill's bundled tests and, if they pass and it is well-formed, generate it for use (register). A contract lint runs first: name/description present, any scripts/<name> the SKILL.md references is bundled, each tests/ file executable. Returns the failure if the lint or tests fail. A skill opting into implicit auto-invocation must have tests."
    )]
    async fn skill_register(
        &self,
        Parameters(p): Parameters<SkillIdParams>,
    ) -> Result<CallToolResult, McpError> {
        envelope_result(register_skill(&self.owox_dir, self.repo_root(), &p.id))
    }

    /// スキルを昇格する (人間ゲート)。人間承認後にだけ使う。
    #[tool(
        name = "skill.promote",
        description = "Promote a registered skill to trusted canon. Use only after a human has approved. Recorded in the decision log and enables auto-invocation for skills that opt into implicit. A draft (unregistered) skill cannot be promoted."
    )]
    async fn skill_promote(
        &self,
        Parameters(p): Parameters<SkillIdParams>,
    ) -> Result<CallToolResult, McpError> {
        envelope_result(promote_skill(
            &self.owox_dir,
            self.repo_root(),
            &today_utc(),
            &p.id,
        ))
    }

    /// スキルの経験メモリ (memory.md) へ追記する。
    #[tool(
        name = "skill.remember",
        description = "Append a lesson to a skill's experience memory (memory.md): what failed, what to change next time. Guides later improvement and stays in the canon only, not generated."
    )]
    async fn skill_remember(
        &self,
        Parameters(p): Parameters<SkillRememberParams>,
    ) -> Result<CallToolResult, McpError> {
        envelope_result(remember(&self.owox_dir, &today_utc(), &p.id, &p.text))
    }

    /// canon (brand / rules / practices / glossary) へ項目を追加する。追加は AI 直接 + 来歴。
    #[tool(
        name = "canon.add",
        description = "Add one item to the project canon (target: brand, rules, practices, or glossary). Adding is AI-direct and recorded; changing or removing existing items is a human gate via canon.propose. For brand and rules also give section (which list to append to); if missing or unknown the response lists valid sections. For glossary set text to \"term: definition\". For practices set text to the practice grown from experience."
    )]
    async fn canon_add(
        &self,
        Parameters(p): Parameters<CanonAddParams>,
    ) -> Result<CallToolResult, McpError> {
        envelope_result(owox_core::canon_add(
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
        description = "Propose changing or removing part of the canon (target: brand, rules, practices, or glossary). A decision for a human; never edits directly. To remove or replace one item, pass op=remove (with item) or op=replace (with item and to); owox applies it after a human approves the gate. For brand and rules also give section; if item is not found the response lists current items. For edits that are not a single list item, pass change as free text. To only add an item, use canon.add."
    )]
    async fn canon_propose(
        &self,
        Parameters(p): Parameters<CanonProposeParams>,
    ) -> Result<CallToolResult, McpError> {
        envelope_result(owox_core::canon_propose(
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
        description = "Export generic experience (skills' SKILL.md + scripts, and practices) to out_path as a portable bundle. Domain-specific parts (memory, tests, owox.toml, brand-fixed canon) are excluded by type. Scans for secrets and refuses to write if any are found; otherwise writes and returns needs_human for review before sharing externally."
    )]
    async fn experience_export(
        &self,
        Parameters(p): Parameters<ExperienceExportParams>,
    ) -> Result<CallToolResult, McpError> {
        envelope_result(owox_core::experience_export(
            &self.owox_dir,
            std::path::Path::new(&p.out_path),
        ))
    }

    /// 別プロジェクトの経験束を取り込む。秘密検出時は人間ゲートで止める。
    #[tool(
        name = "experience.import",
        description = "Import a generic experience bundle from in_path: skills are written as drafts (re-run their tests and promote them here) and practices are merged. Scans for secrets and refuses to import, returning needs_human, if any are found."
    )]
    async fn experience_import(
        &self,
        Parameters(p): Parameters<ExperienceImportParams>,
    ) -> Result<CallToolResult, McpError> {
        envelope_result(owox_core::experience_import(
            &self.owox_dir,
            std::path::Path::new(&p.in_path),
        ))
    }

    /// 用語の定義を引く。canon を直読みせずここから取る。
    #[tool(
        name = "glossary.lookup",
        description = "Look up the project-specific definition of a term. Use instead of reading the canon directly; returns found=false when the term has no project meaning."
    )]
    async fn glossary_lookup(
        &self,
        Parameters(p): Parameters<GlossaryTermParams>,
    ) -> Result<CallToolResult, McpError> {
        envelope_result(glossary_lookup(&self.owox_dir, &p.term))
    }

    /// 運用指針を語で引く。床が肥大化で縮んだ後でも古い指針を取り出せる。
    #[tool(
        name = "practice.lookup",
        description = "Search this project's operating practices for a keyword (matched against the practice text). Use to recall a practice that is not shown in the session context, which happens when many practices exist and only the most recent are listed. An empty query returns all practices, newest first."
    )]
    async fn practice_lookup(
        &self,
        Parameters(p): Parameters<PracticeLookupParams>,
    ) -> Result<CallToolResult, McpError> {
        envelope_result(owox_core::practice_lookup(&self.owox_dir, &p.query))
    }

    /// 調査知識を記録する。要約・出典を秘密走査し、supersedes 指定で旧を置き換える。
    #[tool(
        name = "knowledge.add",
        description = "Record durable research knowledge in .owox/knowledge/: a finding with summary, sources, research date, and optional tags. Use for results of investigating how something works (an API, library, protocol), not design decisions (use decision.record) or operating practices (use canon.add target=practices). Updates are supersede-only: to revise a prior entry, add a new one with supersedes set to its id. Summary and sources are scanned for secrets and refused if found. Read it back with knowledge.lookup and knowledge.get, never .owox/ directly."
    )]
    async fn knowledge_add(
        &self,
        Parameters(p): Parameters<KnowledgeAddParams>,
    ) -> Result<CallToolResult, McpError> {
        envelope_result(owox_core::add_knowledge(
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
        description = "List research knowledge entries with id, title, research date, tags, status (current/superseded), and a stale flag (current entries past the freshness threshold). Optionally filter by status or to stale only. Use knowledge.get to read one in full."
    )]
    async fn knowledge_list(
        &self,
        Parameters(p): Parameters<KnowledgeListParams>,
    ) -> Result<CallToolResult, McpError> {
        let stale_days = owox_core::load_canon(&self.owox_dir)
            .map(|c| c.quality.decay.knowledge_stale_days)
            .unwrap_or(90);
        envelope_result(owox_core::list_knowledge_envelope(
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
        description = "Get one research knowledge entry in full: summary, sources, research date, tags, status, and what it supersedes. Use instead of reading .owox/ directly."
    )]
    async fn knowledge_get(
        &self,
        Parameters(p): Parameters<KnowledgeGetParams>,
    ) -> Result<CallToolResult, McpError> {
        envelope_result(owox_core::get_knowledge(&self.owox_dir, &p.id))
    }

    /// 調査知識を語で引く。title / summary / tags に部分一致するものを返す。
    #[tool(
        name = "knowledge.lookup",
        description = "Search recorded research knowledge for a keyword (matched against title, summary, and tags) and get the matching entries' summaries. Use on demand before re-investigating something, instead of reading .owox/ directly."
    )]
    async fn knowledge_lookup(
        &self,
        Parameters(p): Parameters<KnowledgeLookupParams>,
    ) -> Result<CallToolResult, McpError> {
        envelope_result(owox_core::lookup_knowledge(&self.owox_dir, &p.query))
    }

    /// プロジェクトの rules / policy をまとめて引く。canon を直読みせずここから取る。
    ///
    /// 床コンテキストは rules 本文を常時載せない (最小コンテキスト)。語トリガ push が外した時の
    /// backstop として AI が能動的に引ける (glossary.lookup と対称)。封筒でなく描画本文を返す。
    #[tool(
        name = "rules.lookup",
        description = "Get the project's rules and policies: change, dependency, and deletion policy, safety, irreversible operations, and when to hand work back to a human. Use instead of reading .owox/ directly."
    )]
    async fn rules_lookup(&self) -> Result<CallToolResult, McpError> {
        let canon = owox_core::load_canon(&self.owox_dir)
            .map_err(|err| McpError::internal_error(format!("正本を読めない: {err}"), None))?;
        Ok(CallToolResult::success(vec![Content::text(
            owox_core::render_rules_block(&canon.rules),
        )]))
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

/// 封筒を JSON 文字列にしてテキストの tool 結果へ詰める。
///
/// structured_content でなくテキストにするのは Codex の対応が不確実なため
/// (`docs/decisions/20260613-Phase4-tool記録層.md`)。
fn envelope_result(envelope: Envelope) -> Result<CallToolResult, McpError> {
    let json = serde_json::to_string_pretty(&envelope)
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

/// 文脈地図を Markdown へ描画する。作業/パスごとに「読む先 + 適用ルール」を示す。
///
/// 末尾に推定トークン数を 1 行足し、owox が注入する情報量を毎回数値で見せる
/// (旗「最小コンテキスト」の測定可能化)。同じ本文を秘密走査し、当たれば警告行を足す
/// (read 専用ナビなので block せず気づかせる。`docs/decisions/20260614-Phase7-測定可視化とブランド検証.md`)。
fn render_context(context: &owox_core::Context) -> String {
    let mut out = if context.entries.is_empty() {
        String::from("# Context map\n\nNo entries yet. Read files under .owox/ directly.\n")
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
    axes: owox_core::Axes,
    auto: AutoApproval,
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

    if open.is_empty()
        && ready.is_empty()
        && untraced.is_empty()
        && decay.is_empty()
        && routines.is_empty()
        && unprioritized.is_empty()
        && auto_pending.is_empty()
        && !auto.active()
    {
        return "# What to decide next\n\nNothing is open, no task is ready, every accepted requirement has a verification trace, and nothing is decaying.\n"
            .to_string();
    }

    let mut out = String::from("# What to decide next\n\n");
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
            "- {} (seen {}x)\n",
            r.sequence.join(" → "),
            r.occurrences
        ));
    }
    if routines.len() > SHOWN {
        out.push_str(&format!("- … and {} more\n", routines.len() - SHOWN));
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
            owox_core::Axes::default(),
            AutoApproval {
                profile: false,
                session: false,
            },
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
            owox_core::Axes::default(),
            AutoApproval {
                profile: false,
                session: true,
            },
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
            flat,
            AutoApproval {
                profile: true,
                session: false,
            },
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
            owox_core::Axes::default(),
            AutoApproval {
                profile: false,
                session: false,
            },
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
            owox_core::Axes::default(),
            AutoApproval {
                profile: false,
                session: true,
            },
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
            phased,
            AutoApproval {
                profile: false,
                session: false,
            },
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
            continuous,
            AutoApproval {
                profile: false,
                session: false,
            },
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
            layered,
            AutoApproval {
                profile: false,
                session: false,
            },
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
            flat,
            AutoApproval {
                profile: false,
                session: false,
            },
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
            ideal,
            AutoApproval {
                profile: false,
                session: false,
            },
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
            incremental,
            AutoApproval {
                profile: false,
                session: false,
            },
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
            owox_core::Axes::default(),
            AutoApproval {
                profile: false,
                session: false,
            },
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
            owox_core::Axes::default(),
            AutoApproval {
                profile: false,
                session: false,
            },
        );
        assert!(out.contains("and 4 more"));
    }

    #[test]
    fn routine_section_lists_suggestions() {
        let routines = vec![owox_core::RoutineSuggestion {
            sequence: vec!["task.create".to_string(), "task.note".to_string()],
            occurrences: 6,
        }];
        let out = render_next(
            &[],
            &[],
            &[],
            &[],
            &routines,
            owox_core::Axes::default(),
            AutoApproval {
                profile: false,
                session: false,
            },
        );
        assert!(out.contains("Routines you could grow into a skill"));
        assert!(out.contains("task.create → task.note"));
        assert!(out.contains("seen 6x"));
    }
}
