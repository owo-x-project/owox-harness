//! 検証可能タスク層: `.owox/tasks/<id>.md` に 1 タスク 1 ファイル (来歴と同形式)。
//!
//! 腐敗を防ぐ中核 (`docs/decisions/20260611-タスク管理.md`): done に検証必須 (自己申告 done を排除)、
//! drop は理由を来歴へ (silent rot 禁止)。ready は前提タスクが done (検証済) まで解決済とみなす。
//! 適応度関数による腐敗検知 (stale / 孤立 / 重複 / ゾンビ) は Phase 7。
//!
//! ID は日付+slug。中央台帳を持たず並行ブランチで衝突しない (来歴と同方針)。

use std::path::{Path, PathBuf};

use serde_json::json;

use crate::envelope::{Envelope, Gate};
use crate::markdown::{Doc, split_pair};
use crate::model::VerifyCheck;
use crate::record::{
    DecisionLinks, DecisionStatus, RecordInput, allocate_id, record_decision, slugify,
};

/// タスクの状態。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskStatus {
    Todo,
    Doing,
    Done,
    Blocked,
    Dropped,
}

impl TaskStatus {
    fn parse(value: &str) -> Result<TaskStatus, String> {
        match value.trim() {
            "todo" => Ok(TaskStatus::Todo),
            "doing" => Ok(TaskStatus::Doing),
            "done" => Ok(TaskStatus::Done),
            "blocked" => Ok(TaskStatus::Blocked),
            "dropped" => Ok(TaskStatus::Dropped),
            other => Err(format!(
                "status は todo / doing / done / blocked / dropped のみ: {other}"
            )),
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            TaskStatus::Todo => "todo",
            TaskStatus::Doing => "doing",
            TaskStatus::Done => "done",
            TaskStatus::Blocked => "blocked",
            TaskStatus::Dropped => "dropped",
        }
    }
}

/// 依存の種類 (型付き)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DepKind {
    /// 前提。target が done になるまでこのタスクは ready にならない。
    Blocks,
    /// 親子。
    ParentChild,
    /// 関連。
    Related,
    /// 作業中に派生して見つかった。
    DiscoveredFrom,
}

impl DepKind {
    pub fn parse(value: &str) -> Result<DepKind, String> {
        match value.trim() {
            "blocks" => Ok(DepKind::Blocks),
            "parent-child" => Ok(DepKind::ParentChild),
            "related" => Ok(DepKind::Related),
            "discovered-from" => Ok(DepKind::DiscoveredFrom),
            other => Err(format!(
                "dep の種類は blocks / parent-child / related / discovered-from のみ: {other}"
            )),
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            DepKind::Blocks => "blocks",
            DepKind::ParentChild => "parent-child",
            DepKind::Related => "related",
            DepKind::DiscoveredFrom => "discovered-from",
        }
    }
}

/// 依存 1 件。`種類: 対象タスク ID`。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dep {
    pub kind: DepKind,
    pub target: String,
}

/// 外部システムへの対応付け 1 件。`システム: 参照` (例: `github: owner/repo#123`)。
///
/// owox が正本で、外部 issue tracker への双方向同期の対応付けを task 側へ持つ。再同期で
/// 重複作成しないための鍵 (`docs/decisions/20260621-Phase9-経験層スケールとGitHub連携とkickoff束ね.md`)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternalRef {
    /// 外部システム名 (例: github)。
    pub system: String,
    /// その系での参照 (例: owner/repo#123)。
    pub reference: String,
}

/// タスクの link 先。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TaskLinks {
    pub requirement: Option<String>,
    pub decision: Option<String>,
    pub verification: Option<String>,
}

impl TaskLinks {
    fn is_empty(&self) -> bool {
        self.requirement.is_none() && self.decision.is_none() && self.verification.is_none()
    }
}

/// タスク 1 件。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Task {
    pub id: String,
    pub title: String,
    pub status: TaskStatus,
    pub links: TaskLinks,
    pub deps: Vec<Dep>,
    /// 一時メモ (作業状態の覚書)。各要素は `<日付>: <本文>`。来歴 (decision) ではない軽量記録。
    pub notes: Vec<String>,
    /// 層タグ (クリーンアーキ)。architecture=layered の時だけ層別報告に使う。
    pub layer: Option<String>,
    /// 段タグ (段階化)。delivery=phased の時だけ stage グルーピングに使う。
    pub stage: Option<String>,
    /// 外部システムへの対応付け (例: github issue)。双方向同期の鍵。
    pub external: Vec<ExternalRef>,
}

/// task.create の入力。
#[derive(Debug, Clone, Default)]
pub struct CreateTaskInput {
    pub title: String,
    pub links: TaskLinks,
    pub deps: Vec<Dep>,
    pub layer: Option<String>,
    pub stage: Option<String>,
    pub external: Vec<ExternalRef>,
}

/// task.update の入力。
#[derive(Debug, Clone, Default)]
pub struct UpdateTaskInput {
    /// 新しいタイトル (内容変更)。変更時は reason 必須・来歴へ記録する。
    pub title: Option<String>,
    /// 新しい状態 (done は task.close を使う)。
    pub status: Option<String>,
    /// link を差し替える。
    pub links: Option<TaskLinks>,
    /// 追加する依存。
    pub add_deps: Vec<Dep>,
    /// title 変更の理由。title を変える時は必須。
    pub reason: Option<String>,
    /// 層タグ (クリーンアーキ)。設定時のみ変える。
    pub layer: Option<String>,
    /// 段タグ (段階化)。設定時のみ変える。
    pub stage: Option<String>,
    /// 追加する外部対応付け。既存と重複する組は足さない (再同期で増えない)。
    pub add_external: Vec<ExternalRef>,
}

impl Task {
    fn render(&self) -> String {
        let mut out = format!("# {}\n\n", self.title);
        out.push_str(&format!("## Status\n\n{}\n\n", self.status.as_str()));

        if !self.links.is_empty() {
            out.push_str("## Links\n\n");
            if let Some(r) = &self.links.requirement {
                out.push_str(&format!("- requirement: {r}\n"));
            }
            if let Some(d) = &self.links.decision {
                out.push_str(&format!("- decision: {d}\n"));
            }
            if let Some(v) = &self.links.verification {
                out.push_str(&format!("- verification: {v}\n"));
            }
            out.push('\n');
        }

        if let Some(l) = &self.layer
            && !l.trim().is_empty()
        {
            out.push_str(&format!("## Layer\n\n{}\n\n", l.trim()));
        }
        if let Some(s) = &self.stage
            && !s.trim().is_empty()
        {
            out.push_str(&format!("## Stage\n\n{}\n\n", s.trim()));
        }

        if !self.deps.is_empty() {
            out.push_str("## Deps\n\n");
            for dep in &self.deps {
                out.push_str(&format!("- {}: {}\n", dep.kind.as_str(), dep.target));
            }
            out.push('\n');
        }

        if !self.external.is_empty() {
            out.push_str("## External\n\n");
            for e in &self.external {
                out.push_str(&format!("- {}: {}\n", e.system, e.reference));
            }
            out.push('\n');
        }

        if !self.notes.is_empty() {
            out.push_str("## Notes\n\n");
            for note in &self.notes {
                out.push_str(&format!("- {note}\n"));
            }
            out.push('\n');
        }

        out
    }

    fn parse(id: &str, text: &str) -> Result<Task, String> {
        let title = text
            .lines()
            .map(str::trim)
            .find_map(|l| l.strip_prefix("# ").filter(|_| !l.starts_with("## ")))
            .unwrap_or("")
            .trim()
            .to_string();

        let mut doc = Doc::parse(text);

        let status = doc
            .take("Status")
            .map(|s| s.text())
            .ok_or_else(|| "Status セクションが必須".to_string())
            .and_then(|t| TaskStatus::parse(&t))?;

        let mut links = TaskLinks::default();
        if let Some(section) = doc.take("Links") {
            for item in section.list() {
                let (key, value) = split_pair(&item);
                match key.as_str() {
                    "requirement" => links.requirement = Some(value),
                    "decision" => links.decision = Some(value),
                    "verification" => links.verification = Some(value),
                    other => return Err(format!("Links の未知のキー: {other}")),
                }
            }
        }

        let mut deps = Vec::new();
        if let Some(section) = doc.take("Deps") {
            for item in section.list() {
                let (kind, target) = split_pair(&item);
                deps.push(Dep {
                    kind: DepKind::parse(&kind)?,
                    target,
                });
            }
        }

        let notes = doc.take("Notes").map(|s| s.list()).unwrap_or_default();

        let mut external = Vec::new();
        if let Some(section) = doc.take("External") {
            for item in section.list() {
                let (system, reference) = split_pair(&item);
                if !system.is_empty() {
                    external.push(ExternalRef { system, reference });
                }
            }
        }

        let layer = doc
            .take("Layer")
            .map(|s| s.text())
            .filter(|t| !t.trim().is_empty());
        let stage = doc
            .take("Stage")
            .map(|s| s.text())
            .filter(|t| !t.trim().is_empty());

        Ok(Task {
            id: id.to_string(),
            title,
            status,
            links,
            deps,
            notes,
            layer,
            stage,
            external,
        })
    }
}

/// `.owox/tasks/`。
fn tasks_dir(owox_dir: &Path) -> PathBuf {
    owox_dir.join("tasks")
}

fn task_path(owox_dir: &Path, id: &str) -> PathBuf {
    tasks_dir(owox_dir).join(format!("{id}.md"))
}

/// 全タスクを読む。`.owox/tasks/*.md`。ディレクトリが無ければ空。
pub fn list_tasks(owox_dir: &Path) -> Result<Vec<Task>, String> {
    let dir = tasks_dir(owox_dir);
    let read = match std::fs::read_dir(&dir) {
        Ok(read) => read,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(format!("{} を読めない: {err}", dir.display())),
    };
    let mut tasks = Vec::new();
    for entry in read {
        let path = entry.map_err(|e| e.to_string())?.path();
        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        let id = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or_default()
            .to_string();
        let text = std::fs::read_to_string(&path)
            .map_err(|e| format!("{} を読めない: {e}", path.display()))?;
        let task = Task::parse(&id, &text)
            .map_err(|e| format!("{} を解釈できない: {e}", path.display()))?;
        tasks.push(task);
    }
    tasks.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(tasks)
}

/// タスクが ready か。todo で、前提 (blocks) の対象がすべて done (検証済) なら ready。
///
/// done は close が検証を通した状態なので、done = 検証済とみなせる
/// (自己申告 done を排除する設計。`docs/decisions/20260611-タスク管理.md`)。
pub fn is_ready(task: &Task, all: &[Task]) -> bool {
    if task.status != TaskStatus::Todo {
        return false;
    }
    task.deps
        .iter()
        .filter(|d| d.kind == DepKind::Blocks)
        .all(|d| {
            all.iter()
                .find(|t| t.id == d.target)
                .is_some_and(|t| t.status == TaskStatus::Done)
        })
}

/// deps の target に実在しないタスクがあれば、その最初の ID を返す。全て実在なら None。
/// 読めない時は検証を諦め None (作業を妨げない。重複・壊れ検知は Phase 7)。
fn unknown_dep_target(owox_dir: &Path, deps: &[Dep]) -> Option<String> {
    if deps.is_empty() {
        return None;
    }
    let existing = list_tasks(owox_dir).ok()?;
    deps.iter()
        .find(|d| !existing.iter().any(|t| t.id == d.target))
        .map(|d| d.target.clone())
}

/// task.create。`.owox/tasks/<id>.md` を書く。
pub fn create_task(
    owox_dir: &Path,
    today: &str,
    known_layers: &[String],
    input: CreateTaskInput,
) -> Envelope {
    if input.title.trim().is_empty() {
        return Envelope::failed("title が空");
    }
    if let Some(l) = &input.layer
        && let Err(err) = crate::quality::check_known_layer(l, known_layers)
    {
        return Envelope::failed(err);
    }
    let dir = tasks_dir(owox_dir);
    if let Err(err) = std::fs::create_dir_all(&dir) {
        return Envelope::failed(format!("{} を作れない: {err}", dir.display()));
    }

    // 依存先が実在するか確認する。存在しない target を指す壊れた依存グラフを防ぐ。
    // 前方参照 (まだ無いタスクへの依存) は受けない。後から張る時は task.link を使う。
    if let Some(unknown) = unknown_dep_target(owox_dir, &input.deps) {
        return Envelope::failed(format!(
            "Unknown dependency target: {unknown}. Create that task first, or add the dependency later with task.link."
        ));
    }

    let id = allocate_id(&dir, today, &slugify(&input.title));
    let task = Task {
        id: id.clone(),
        title: input.title,
        status: TaskStatus::Todo,
        links: input.links,
        deps: input.deps,
        notes: Vec::new(),
        layer: input.layer.filter(|s| !s.trim().is_empty()),
        stage: input.stage.filter(|s| !s.trim().is_empty()),
        external: input.external,
    };
    if let Err(err) = std::fs::write(task_path(owox_dir, &id), task.render()) {
        return Envelope::failed(format!("タスクを書けない: {err}"));
    }
    Envelope::ok("Created the task.", json!({ "id": id }))
}

/// task.list。ready / status で絞って返す。
pub fn list_tasks_envelope(owox_dir: &Path, ready_only: bool, status: Option<&str>) -> Envelope {
    let tasks = match list_tasks(owox_dir) {
        Ok(t) => t,
        Err(err) => return Envelope::failed(err),
    };
    let status_filter = match status.map(TaskStatus::parse).transpose() {
        Ok(s) => s,
        Err(err) => return Envelope::failed(err),
    };

    let listed: Vec<_> = tasks
        .iter()
        .filter(|t| !ready_only || is_ready(t, &tasks))
        .filter(|t| status_filter.is_none_or(|s| t.status == s))
        .map(|t| {
            json!({
                "id": t.id,
                "title": t.title,
                "status": t.status.as_str(),
                "ready": is_ready(t, &tasks),
                "deps": t.deps.iter().map(|d| json!({ "kind": d.kind.as_str(), "target": d.target })).collect::<Vec<_>>(),
                "external": t.external.iter().map(|e| json!({ "system": e.system, "reference": e.reference })).collect::<Vec<_>>(),
            })
        })
        .collect();

    Envelope::ok(
        format!("{} task(s).", listed.len()),
        json!({ "tasks": listed }),
    )
}

/// 既存タスクを読む。無ければ Err 文言。
fn load_task(owox_dir: &Path, id: &str) -> Result<Task, String> {
    let path = task_path(owox_dir, id);
    match std::fs::read_to_string(&path) {
        Ok(text) => Task::parse(id, &text),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            Err(format!("タスクが無い: {id}"))
        }
        Err(err) => Err(format!("{} を読めない: {err}", path.display())),
    }
}

fn save_task(owox_dir: &Path, task: &Task) -> Result<(), String> {
    std::fs::write(task_path(owox_dir, &task.id), task.render())
        .map_err(|e| format!("タスクを書けない: {e}"))
}

/// task.update。status / links / deps の変更は軽量 (来歴なし)。
/// title (タスクが何であるか) の変更だけは内容変更とみなし reason 必須・来歴へ残す。
///
/// status / links / deps まで来歴にすると過剰記録に逆戻りするため分ける
/// (`docs/handoff/20260613-Phase4対話検証で見つけた粗の改善.md`)。done への遷移は受けない (close 経由)。
pub fn update_task(
    owox_dir: &Path,
    today: &str,
    id: &str,
    known_layers: &[String],
    input: UpdateTaskInput,
) -> Envelope {
    let mut task = match load_task(owox_dir, id) {
        Ok(t) => t,
        Err(err) => return Envelope::failed(err),
    };

    if let Some(l) = &input.layer
        && let Err(err) = crate::quality::check_known_layer(l, known_layers)
    {
        return Envelope::failed(err);
    }

    // done への遷移は close を通す (検証必須)。update では受けない。
    if let Some(s) = &input.status {
        match TaskStatus::parse(s) {
            Ok(TaskStatus::Done) => {
                return Envelope::failed("done への遷移は task.close を使う (検証必須)");
            }
            Ok(parsed) => task.status = parsed,
            Err(err) => return Envelope::failed(err),
        }
    }
    if let Some(l) = input.links {
        task.links = l;
    }
    task.deps.extend(input.add_deps);

    // title (内容) 変更は reason 必須・来歴へ。links 差し替えの後に decision link を載せる。
    let mut decision_ids = Vec::new();
    if let Some(new_title) = input.title.as_deref().map(str::trim)
        && !new_title.is_empty()
        && new_title != task.title
    {
        let reason = input.reason.as_deref().unwrap_or("").trim();
        if reason.is_empty() {
            return Envelope::failed(
                "Changing a task's title is a content change; provide a reason (it is recorded as a decision).",
            );
        }
        let record = record_decision(
            owox_dir,
            today,
            RecordInput {
                title: format!("Update task title: {} → {new_title}", task.title),
                status: DecisionStatus::Adopted,
                rationale: reason.to_string(),
                links: DecisionLinks::default(),
                supersedes: Vec::new(),
            },
        );
        task.links.decision = record
            .data
            .as_ref()
            .and_then(|d| d["id"].as_str())
            .map(String::from);
        decision_ids = record.decision_ids;
        task.title = new_title.to_string();
    }

    // 外部対応付けの追加 (軽量変更)。既存と重複する組は足さない (再同期で増えない)。
    for e in input.add_external {
        if !task.external.contains(&e) {
            task.external.push(e);
        }
    }

    // 属性 (層/段) は軽量変更で来歴連動しない。
    if let Some(l) = input.layer {
        let l = l.trim();
        task.layer = (!l.is_empty()).then(|| l.to_string());
    }
    if let Some(s) = input.stage {
        let s = s.trim();
        task.stage = (!s.is_empty()).then(|| s.to_string());
    }

    if let Err(err) = save_task(owox_dir, &task) {
        return Envelope::failed(err);
    }
    Envelope::ok("Updated the task.", json!({ "id": id })).with_decision_ids(decision_ids)
}

/// task.note。タスクへ一時メモを 1 件追記する (来歴ではない軽量記録)。
///
/// 作業状態の覚書を decision でなくここへ逃がし、来歴の乱立を防ぐ
/// (`docs/handoff/20260613-Phase4対話検証で見つけた粗の改善.md`)。
pub fn add_note(owox_dir: &Path, today: &str, id: &str, text: &str) -> Envelope {
    if text.trim().is_empty() {
        return Envelope::failed("note text is empty");
    }
    let mut task = match load_task(owox_dir, id) {
        Ok(t) => t,
        Err(err) => return Envelope::failed(err),
    };
    task.notes.push(format!("{today}: {}", text.trim()));
    if let Err(err) = save_task(owox_dir, &task) {
        return Envelope::failed(err);
    }
    Envelope::ok("Added a note to the task.", json!({ "id": id }))
}

/// task.link。依存を 1 件足す。
pub fn link_task(owox_dir: &Path, id: &str, dep: Dep) -> Envelope {
    let mut task = match load_task(owox_dir, id) {
        Ok(t) => t,
        Err(err) => return Envelope::failed(err),
    };
    task.deps.push(dep);
    if let Err(err) = save_task(owox_dir, &task) {
        return Envelope::failed(err);
    }
    Envelope::ok("Linked the dependency.", json!({ "id": id }))
}

/// task.close。done に検証必須。検査を実行し通過なら done、失敗は failed、未設定は needs_human。
///
/// 完了3区別の検証 (機械) を close の前提にして自己申告 done を排除する
/// (`docs/decisions/20260611-タスク管理.md`)。work_dir は検査を走らせる target repo ルート。
pub fn close_task(
    owox_dir: &Path,
    work_dir: &Path,
    checks: &[VerifyCheck],
    today: &str,
    id: &str,
) -> Envelope {
    let mut task = match load_task(owox_dir, id) {
        Ok(t) => t,
        Err(err) => return Envelope::failed(err),
    };

    if checks.is_empty() {
        return Envelope::needs_human(
            "No verification checks are configured, so this task cannot be machine-verified before closing. Confirm completion, or add [[verify.checks]] to config.toml.",
            Gate {
                kind: "completion-judgment".to_string(),
                subject: format!("close task {id} (no checks configured)"),
                requires: "Confirm the task is complete, or configure checks.".to_string(),
            },
        );
    }

    let results = crate::verify::run_checks(work_dir, checks);
    let failed: Vec<String> = results
        .iter()
        .filter(|r| !r.passed)
        .map(|r| r.name.clone())
        .collect();
    if !failed.is_empty() {
        return Envelope::failed(format!(
            "Cannot close: verification failed. Failing checks: {}.",
            failed.join(", ")
        ))
        .with_next_actions(vec!["Fix the failing checks and close again.".to_string()]);
    }

    task.status = TaskStatus::Done;
    task.links.verification = Some(format!("checks passed {today}"));
    if let Err(err) = save_task(owox_dir, &task) {
        return Envelope::failed(err);
    }
    Envelope::ok(
        "Closed the task (verification passed).",
        json!({ "id": id, "closed": true }),
    )
}

/// task.drop。理由を来歴へ残し、status を dropped にする (silent rot 禁止)。
pub fn drop_task(owox_dir: &Path, today: &str, id: &str, reason: &str) -> Envelope {
    if reason.trim().is_empty() {
        return Envelope::failed("drop には理由が必須");
    }
    let mut task = match load_task(owox_dir, id) {
        Ok(t) => t,
        Err(err) => return Envelope::failed(err),
    };

    // 破棄の理由を来歴へ (追跡可能にする)。
    let record = record_decision(
        owox_dir,
        today,
        RecordInput {
            title: format!("Drop task: {}", task.title),
            status: DecisionStatus::Adopted,
            rationale: reason.to_string(),
            links: DecisionLinks::default(),
            supersedes: Vec::new(),
        },
    );

    task.status = TaskStatus::Dropped;
    task.links.decision = record
        .data
        .as_ref()
        .and_then(|d| d["id"].as_str())
        .map(String::from);
    if let Err(err) = save_task(owox_dir, &task) {
        return Envelope::failed(err);
    }
    Envelope::ok(
        "Dropped the task. The reason is recorded.",
        json!({ "id": id, "dropped": true }),
    )
    .with_decision_ids(record.decision_ids)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::envelope::Status;

    fn tempdir() -> PathBuf {
        use std::sync::atomic::{AtomicU32, Ordering};
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let pid = std::process::id();
        let dir = std::env::temp_dir().join(format!("owox-task-test-{pid}-{n}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn create(dir: &Path, title: &str) -> String {
        let env = create_task(
            dir,
            "20260613",
            &[],
            CreateTaskInput {
                title: title.to_string(),
                ..CreateTaskInput::default()
            },
        );
        env.data.unwrap()["id"].as_str().unwrap().to_string()
    }

    fn check(cmd: &str) -> Vec<VerifyCheck> {
        vec![VerifyCheck {
            name: cmd.to_string(),
            command: cmd.to_string(),
            evidence_paths: Vec::new(),
        }]
    }

    #[test]
    fn create_then_roundtrip() {
        let dir = tempdir();
        let id = create(&dir, "Write the parser");
        let tasks = list_tasks(&dir).unwrap();
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].id, id);
        assert_eq!(tasks[0].title, "Write the parser");
        assert_eq!(tasks[0].status, TaskStatus::Todo);
    }

    #[test]
    fn external_refs_roundtrip_and_dedup() {
        let dir = tempdir();
        let env = create_task(
            &dir,
            "20260621",
            &[],
            CreateTaskInput {
                title: "Wire the port".to_string(),
                external: vec![ExternalRef {
                    system: "github".to_string(),
                    reference: "owner/repo#12".to_string(),
                }],
                ..CreateTaskInput::default()
            },
        );
        let id = env.data.unwrap()["id"].as_str().unwrap().to_string();
        // 永続して読み戻せる。
        let t = load_task(&dir, &id).unwrap();
        assert_eq!(t.external.len(), 1);
        assert_eq!(t.external[0].reference, "owner/repo#12");
        // 同じ組の再追加は増えない (再同期で重複しない)。別の組は足す。
        update_task(
            &dir,
            "20260621",
            &id,
            &[],
            UpdateTaskInput {
                add_external: vec![
                    ExternalRef {
                        system: "github".to_string(),
                        reference: "owner/repo#12".to_string(),
                    },
                    ExternalRef {
                        system: "github".to_string(),
                        reference: "owner/repo#99".to_string(),
                    },
                ],
                ..UpdateTaskInput::default()
            },
        );
        let t = load_task(&dir, &id).unwrap();
        assert_eq!(t.external.len(), 2);
    }

    #[test]
    fn layer_stage_roundtrip_and_update() {
        let dir = tempdir();
        let env = create_task(
            &dir,
            "20260618",
            &[],
            CreateTaskInput {
                title: "Wire the core".to_string(),
                layer: Some("core".to_string()),
                stage: Some("mvp".to_string()),
                ..CreateTaskInput::default()
            },
        );
        let id = env.data.unwrap()["id"].as_str().unwrap().to_string();
        let t = load_task(&dir, &id).unwrap();
        assert_eq!(t.layer.as_deref(), Some("core"));
        assert_eq!(t.stage.as_deref(), Some("mvp"));
        // 空文字で stage を消す。
        update_task(
            &dir,
            "20260618",
            &id,
            &[],
            UpdateTaskInput {
                stage: Some("".to_string()),
                ..UpdateTaskInput::default()
            },
        );
        assert_eq!(load_task(&dir, &id).unwrap().stage, None);
    }

    #[test]
    fn create_validates_layer_against_declared_layers() {
        let dir = tempdir();
        let declared = vec!["core".to_string()];
        // 宣言済の層名なら通る。
        let env = create_task(
            &dir,
            "20260618",
            &declared,
            CreateTaskInput {
                title: "ok".to_string(),
                layer: Some("core".to_string()),
                ..CreateTaskInput::default()
            },
        );
        assert_eq!(env.status, Status::Ok);
        // 未知 layer は弾く。
        let env = create_task(
            &dir,
            "20260618",
            &declared,
            CreateTaskInput {
                title: "ng".to_string(),
                layer: Some("ghost".to_string()),
                ..CreateTaskInput::default()
            },
        );
        assert_eq!(env.status, Status::Failed);
    }

    #[test]
    fn ready_respects_blocking_deps() {
        let dir = tempdir();
        let a = create(&dir, "prerequisite");
        let b = create(&dir, "dependent");
        link_task(
            &dir,
            &b,
            Dep {
                kind: DepKind::Blocks,
                target: a.clone(),
            },
        );

        let tasks = list_tasks(&dir).unwrap();
        let task_b = tasks.iter().find(|t| t.id == b).unwrap();
        // a が未 done なので b は ready でない。
        assert!(!is_ready(task_b, &tasks));

        // a を done にすると b が ready。
        close_task(&dir, &dir, &check("true"), "20260613", &a);
        let tasks = list_tasks(&dir).unwrap();
        let task_b = tasks.iter().find(|t| t.id == b).unwrap();
        assert!(is_ready(task_b, &tasks));
    }

    #[test]
    fn close_requires_passing_checks() {
        let dir = tempdir();
        let id = create(&dir, "do the thing");
        // 失敗検査 → close できない。
        let env = close_task(&dir, &dir, &check("false"), "20260613", &id);
        assert_eq!(env.status, Status::Failed);
        assert_eq!(list_tasks(&dir).unwrap()[0].status, TaskStatus::Todo);
        // 通過 → done。
        let env = close_task(&dir, &dir, &check("true"), "20260613", &id);
        assert_eq!(env.status, Status::Ok);
        assert_eq!(list_tasks(&dir).unwrap()[0].status, TaskStatus::Done);
    }

    #[test]
    fn close_without_checks_needs_human() {
        let dir = tempdir();
        let id = create(&dir, "x");
        let env = close_task(&dir, &dir, &[], "20260613", &id);
        assert_eq!(env.status, Status::NeedsHuman);
    }

    #[test]
    fn drop_requires_reason_and_records_decision() {
        let dir = tempdir();
        let id = create(&dir, "obsolete task");
        assert_eq!(drop_task(&dir, "20260613", &id, "").status, Status::Failed);
        let env = drop_task(&dir, "20260613", &id, "no longer needed");
        assert_eq!(env.status, Status::Ok);
        assert_eq!(env.decision_ids.len(), 1);
        assert_eq!(list_tasks(&dir).unwrap()[0].status, TaskStatus::Dropped);
        // 破棄理由が来歴に残る。
        let decisions = crate::record::list_decisions(&dir).unwrap();
        assert!(
            decisions
                .iter()
                .any(|d| d.rationale.contains("no longer needed"))
        );
    }

    #[test]
    fn update_rejects_done_transition() {
        let dir = tempdir();
        let id = create(&dir, "t");
        assert_eq!(
            update_task(
                &dir,
                "20260613",
                &id,
                &[],
                UpdateTaskInput {
                    status: Some("done".to_string()),
                    ..UpdateTaskInput::default()
                }
            )
            .status,
            Status::Failed
        );
        // doing は通る。来歴は残らない (workflow 変更は軽量)。
        let env = update_task(
            &dir,
            "20260613",
            &id,
            &[],
            UpdateTaskInput {
                status: Some("doing".to_string()),
                ..UpdateTaskInput::default()
            },
        );
        assert_eq!(env.status, Status::Ok);
        assert!(env.decision_ids.is_empty());
    }

    #[test]
    fn create_rejects_unknown_dep_target() {
        let dir = tempdir();
        // 実在しない target への依存は弾く。
        let env = create_task(
            &dir,
            "20260613",
            &[],
            CreateTaskInput {
                title: "dependent".to_string(),
                deps: vec![Dep {
                    kind: DepKind::Blocks,
                    target: "20260613-nonexistent".to_string(),
                }],
                ..CreateTaskInput::default()
            },
        );
        assert_eq!(env.status, Status::Failed);
        assert!(list_tasks(&dir).unwrap().is_empty());

        // 実在する target なら通る。
        let a = create(&dir, "prerequisite");
        let env = create_task(
            &dir,
            "20260613",
            &[],
            CreateTaskInput {
                title: "dependent".to_string(),
                deps: vec![Dep {
                    kind: DepKind::Blocks,
                    target: a,
                }],
                ..CreateTaskInput::default()
            },
        );
        assert_eq!(env.status, Status::Ok);
    }

    #[test]
    fn note_appends_and_roundtrips() {
        let dir = tempdir();
        let id = create(&dir, "t");
        assert_eq!(add_note(&dir, "20260613", &id, "").status, Status::Failed);
        assert_eq!(
            add_note(&dir, "20260613", &id, "checked the edge case").status,
            Status::Ok
        );
        let task = list_tasks(&dir).unwrap().into_iter().next().unwrap();
        assert_eq!(
            task.notes,
            vec!["20260613: checked the edge case".to_string()]
        );
        // note は来歴を作らない。
        assert!(crate::record::list_decisions(&dir).unwrap().is_empty());
    }

    #[test]
    fn title_change_requires_reason_and_records_decision() {
        let dir = tempdir();
        let id = create(&dir, "old title");
        // 理由なしの title 変更は弾く。
        let env = update_task(
            &dir,
            "20260613",
            &id,
            &[],
            UpdateTaskInput {
                title: Some("new title".to_string()),
                ..UpdateTaskInput::default()
            },
        );
        assert_eq!(env.status, Status::Failed);
        // 理由つきなら通り、来歴へ残る。
        let env = update_task(
            &dir,
            "20260613",
            &id,
            &[],
            UpdateTaskInput {
                title: Some("new title".to_string()),
                reason: Some("scope clarified".to_string()),
                ..UpdateTaskInput::default()
            },
        );
        assert_eq!(env.status, Status::Ok);
        assert_eq!(env.decision_ids.len(), 1);
        let task = list_tasks(&dir).unwrap().into_iter().next().unwrap();
        assert_eq!(task.title, "new title");
        assert!(task.links.decision.is_some());
        let decisions = crate::record::list_decisions(&dir).unwrap();
        assert!(
            decisions
                .iter()
                .any(|d| d.rationale.contains("scope clarified"))
        );
    }
}
