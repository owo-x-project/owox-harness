//! rules / practices の配送判定。
//!
//! 正本本文は既存 model が読むが、配送用 trigger 属性はここで軽く読む。
//! 永続 model を広げず、必要な時だけ rules / practices を届けるための層。

use std::path::Path;

use crate::model::Phase;
use crate::quality::glob_to_regex;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeliverySurface {
    Rules,
    Practices,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum DeliveryOperation {
    Read,
    Edit,
    Delete,
    Commit,
    Review,
    Verify,
    CanonChange,
    DependencyChange,
    RequirementChange,
    SkillChange,
}

impl DeliveryOperation {
    pub fn parse(value: &str) -> Result<Self, String> {
        match value.trim() {
            "read" => Ok(Self::Read),
            "edit" => Ok(Self::Edit),
            "delete" => Ok(Self::Delete),
            "commit" => Ok(Self::Commit),
            "review" => Ok(Self::Review),
            "verify" => Ok(Self::Verify),
            "canon-change" => Ok(Self::CanonChange),
            "dependency-change" => Ok(Self::DependencyChange),
            "requirement-change" => Ok(Self::RequirementChange),
            "skill-change" => Ok(Self::SkillChange),
            other => Err(format!("未知の operation: {other}")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TriggerKind {
    Always,
    Path,
    Operation,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DeliveryEntry {
    surface: DeliverySurface,
    text: String,
    order: usize,
    triggers: Vec<TriggerKind>,
    operations: Vec<DeliveryOperation>,
    paths: Vec<String>,
    phase: Option<Phase>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DeliverySelection {
    pub rules: Vec<String>,
    pub practices: Vec<String>,
}

pub struct DeliveryRequest<'a> {
    pub operations: &'a [DeliveryOperation],
    pub paths: &'a [String],
    pub phase: Option<Phase>,
    pub include_always: bool,
    pub include_path: bool,
    pub include_operation: bool,
    pub always_limit: usize,
}

impl<'a> DeliveryRequest<'a> {
    /// SessionStart 用リクエスト。always_limit は quality.toml の delivery.always_limit (既定 3)。
    pub fn session_start(phase: Phase) -> Self {
        Self::session_start_with_limit(phase, 3)
    }

    /// SessionStart 用リクエスト (always_limit を明示する)。
    pub fn session_start_with_limit(phase: Phase, always_limit: usize) -> Self {
        Self {
            operations: &[],
            paths: &[],
            phase: Some(phase),
            include_always: true,
            include_path: false,
            include_operation: false,
            always_limit,
        }
    }

    pub fn for_paths(paths: &'a [String]) -> Self {
        Self {
            operations: &[],
            paths,
            phase: None,
            include_always: false,
            include_path: true,
            include_operation: false,
            always_limit: 0,
        }
    }

    pub fn for_operations(operations: &'a [DeliveryOperation], paths: &'a [String]) -> Self {
        Self {
            operations,
            paths,
            phase: None,
            include_always: false,
            include_path: true,
            include_operation: true,
            always_limit: 0,
        }
    }
}

pub fn select_delivery(
    owox_dir: &Path,
    req: DeliveryRequest<'_>,
) -> Result<DeliverySelection, String> {
    select_delivery_for_phase(owox_dir, req, Phase::Initial)
}

pub fn select_delivery_for_phase(
    owox_dir: &Path,
    req: DeliveryRequest<'_>,
    phase: Phase,
) -> Result<DeliverySelection, String> {
    use crate::load::load_canon;

    let req = DeliveryRequest {
        phase: req.phase.or(Some(phase)),
        ..req
    };

    let (rule_entries, practice_entries) = match load_canon(owox_dir) {
        Ok(canon) => (canon.rules.entries, canon.practices.rule_entries),
        Err(_) => (Vec::new(), Vec::new()),
    };

    let mut all: Vec<DeliveryEntry> = Vec::new();
    let mut order = 0usize;
    for entry in &rule_entries {
        let mut de = rule_entry_to_delivery(entry)?;
        de.order = order;
        order += 1;
        all.push(de);
    }
    for entry in &practice_entries {
        let mut de = practice_entry_to_delivery(entry)?;
        de.order = order;
        order += 1;
        all.push(de);
    }

    let mut selected = DeliverySelection::default();
    let mut always_used = 0usize;

    for entry in all {
        if !matches_delivery(&entry, &req) {
            continue;
        }
        if req.include_always
            && entry.triggers.contains(&TriggerKind::Always)
            && always_used >= req.always_limit
        {
            continue;
        }
        if req.include_always && entry.triggers.contains(&TriggerKind::Always) {
            always_used += 1;
        }
        match entry.surface {
            DeliverySurface::Rules => selected.rules.push(entry.text),
            DeliverySurface::Practices => selected.practices.push(entry.text),
        }
    }

    Ok(selected)
}

fn rule_entry_to_delivery(entry: &crate::model::RuleEntry) -> Result<DeliveryEntry, String> {
    let triggers = if entry.triggers.is_empty() {
        if entry.paths.is_empty() && entry.operations.is_empty() {
            vec![TriggerKind::Operation]
        } else {
            let mut t = Vec::new();
            if !entry.operations.is_empty() {
                t.push(TriggerKind::Operation);
            }
            if !entry.paths.is_empty() {
                t.push(TriggerKind::Path);
            }
            t
        }
    } else {
        entry
            .triggers
            .iter()
            .map(|s| match s.as_str() {
                "always" => Ok(TriggerKind::Always),
                "path" => Ok(TriggerKind::Path),
                "operation" => Ok(TriggerKind::Operation),
                other => Err(format!("未知の trigger: {other}")),
            })
            .collect::<Result<Vec<_>, _>>()?
    };

    let operations = entry
        .operations
        .iter()
        .map(|v| DeliveryOperation::parse(v))
        .collect::<Result<Vec<_>, _>>()?;

    let text = if entry.section.is_empty() {
        entry.text.clone()
    } else {
        format!("{}: {}", entry.section, entry.text)
    };

    Ok(DeliveryEntry {
        surface: DeliverySurface::Rules,
        text,
        order: 0,
        triggers,
        operations,
        paths: entry.paths.clone(),
        phase: entry.phase,
    })
}

fn practice_entry_to_delivery(
    entry: &crate::model::PracticeEntry,
) -> Result<DeliveryEntry, String> {
    let triggers = if entry.triggers.is_empty() {
        if entry.paths.is_empty() && entry.operations.is_empty() {
            vec![TriggerKind::Path]
        } else {
            let mut t = Vec::new();
            if !entry.operations.is_empty() {
                t.push(TriggerKind::Operation);
            }
            if !entry.paths.is_empty() {
                t.push(TriggerKind::Path);
            }
            t
        }
    } else {
        entry
            .triggers
            .iter()
            .map(|s| match s.as_str() {
                "always" => Ok(TriggerKind::Always),
                "path" => Ok(TriggerKind::Path),
                "operation" => Ok(TriggerKind::Operation),
                other => Err(format!("未知の trigger: {other}")),
            })
            .collect::<Result<Vec<_>, _>>()?
    };

    let operations = entry
        .operations
        .iter()
        .map(|v| DeliveryOperation::parse(v))
        .collect::<Result<Vec<_>, _>>()?;

    Ok(DeliveryEntry {
        surface: DeliverySurface::Practices,
        text: entry.text.clone(),
        order: 0,
        triggers,
        operations,
        paths: entry.paths.clone(),
        phase: None,
    })
}

pub fn render_delivery_block(selection: &DeliverySelection) -> String {
    let mut out = String::new();
    if !selection.rules.is_empty() {
        out.push_str("## Rules\n\n");
        for item in &selection.rules {
            out.push_str("- ");
            out.push_str(item);
            out.push('\n');
        }
        out.push('\n');
    }
    if !selection.practices.is_empty() {
        out.push_str("## Practices\n\n");
        for item in &selection.practices {
            out.push_str("- ");
            out.push_str(item);
            out.push('\n');
        }
        out.push('\n');
    }
    out
}

fn matches_delivery(entry: &DeliveryEntry, req: &DeliveryRequest<'_>) -> bool {
    let same_phase = entry.phase.is_none() || entry.phase == req.phase;
    let mut matched = false;
    if req.include_always && entry.triggers.contains(&TriggerKind::Always) && same_phase {
        matched = true;
    }
    if req.include_operation
        && entry.triggers.contains(&TriggerKind::Operation)
        && (entry.phase.is_none() || entry.phase == req.phase)
        && matches_operations(entry, req.operations)
    {
        matched = true;
    }
    if req.include_path
        && entry.triggers.contains(&TriggerKind::Path)
        && (entry.phase.is_none() || entry.phase == req.phase)
        && matches_paths(entry, req.paths)
    {
        matched = true;
    }
    matched
}

fn matches_operations(entry: &DeliveryEntry, operations: &[DeliveryOperation]) -> bool {
    if entry.operations.is_empty() {
        return !operations.is_empty();
    }
    operations.iter().any(|op| entry.operations.contains(op))
}

fn matches_paths(entry: &DeliveryEntry, paths: &[String]) -> bool {
    if entry.paths.is_empty() {
        return !paths.is_empty();
    }
    entry.paths.iter().any(|glob| {
        let re = glob_to_regex(glob);
        paths.iter().any(|path| re.is_match(path))
    })
}

/// 依存関係ファイル (Cargo.toml 等) かどうかを判定する。
pub fn is_dependency_path(path: &str) -> bool {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn tempdir() -> PathBuf {
        use std::sync::atomic::{AtomicU32, Ordering};
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let pid = std::process::id();
        let dir = std::env::temp_dir().join(format!("owox-delivery-test-{pid}-{n}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        // load_canon は brand.md 必須
        std::fs::write(dir.join("brand.md"), "## Vision\ntest\n").unwrap();
        dir
    }

    #[test]
    fn session_start_only_returns_explicit_always_up_to_limit() {
        let owox = tempdir();
        std::fs::write(
            owox.join("rules.md"),
            "## Change policy\n- one\ntrigger: always\n- two\ntrigger: always\n- three\ntrigger: always\n- four\ntrigger: always\n",
        )
        .unwrap();
        let selected = select_delivery_for_phase(
            &owox,
            DeliveryRequest::session_start(crate::model::Phase::Stable),
            crate::model::Phase::Stable,
        )
        .unwrap();
        assert_eq!(selected.rules.len(), 3);
        assert_eq!(selected.practices.len(), 0);
    }

    #[test]
    fn inferred_operation_and_path_triggers_work() {
        let owox = tempdir();
        std::fs::write(
            owox.join("rules.md"),
            "## Deletion policy\n- verify before delete\noperation: delete\n",
        )
        .unwrap();
        std::fs::write(
            owox.join("practices.md"),
            "## Practices\n- 20260626: add nearby tests\npath: crates/core/**\n",
        )
        .unwrap();
        let paths = vec!["crates/core/src/hook.rs".to_string()];
        let ops = vec![DeliveryOperation::Delete];
        let selected =
            select_delivery(&owox, DeliveryRequest::for_operations(&ops, &paths)).unwrap();
        assert_eq!(selected.rules.len(), 1);
        assert_eq!(selected.practices.len(), 1);
    }

    #[test]
    fn default_surface_trigger_applies_when_no_attrs_exist() {
        let owox = tempdir();
        std::fs::write(owox.join("rules.md"), "## Safety\n- keep backups\n").unwrap();
        std::fs::write(
            owox.join("practices.md"),
            "## Practices\n- 20260626: small diffs\n",
        )
        .unwrap();
        let paths = vec!["src/main.rs".to_string()];
        let ops = vec![DeliveryOperation::Edit];
        let selected =
            select_delivery(&owox, DeliveryRequest::for_operations(&ops, &paths)).unwrap();
        assert_eq!(selected.rules.len(), 1);
        assert_eq!(selected.practices.len(), 1);
    }

    /// 他 phase (Stable) の rule は Initial の SessionStart に出ない。
    #[test]
    fn session_start_excludes_other_phase_rules() {
        let owox = tempdir();
        std::fs::write(
            owox.join("rules.md"),
            "## Initial\n- initial-only\ntrigger: always\n\
             ## Stable\n- stable-only\ntrigger: always\n\
             ## Common\n- common-always\ntrigger: always\n",
        )
        .unwrap();
        let selected = select_delivery_for_phase(
            &owox,
            DeliveryRequest::session_start(crate::model::Phase::Initial),
            crate::model::Phase::Initial,
        )
        .unwrap();
        let texts: Vec<&str> = selected.rules.iter().map(|s| s.as_str()).collect();
        assert!(
            texts.iter().any(|t| t.contains("initial-only")),
            "Initial rule は含まれるはず: {texts:?}"
        );
        assert!(
            texts.iter().any(|t| t.contains("common-always")),
            "Common rule は含まれるはず: {texts:?}"
        );
        assert!(
            !texts.iter().any(|t| t.contains("stable-only")),
            "Stable rule は出ないはず: {texts:?}"
        );
    }

    /// trigger:operation の common rule は SessionStart に出ない。
    #[test]
    fn session_start_excludes_operation_trigger_rules() {
        let owox = tempdir();
        std::fs::write(
            owox.join("rules.md"),
            "## Common\n- always-rule\ntrigger: always\n- operation-rule\noperation: edit\n",
        )
        .unwrap();
        let selected = select_delivery_for_phase(
            &owox,
            DeliveryRequest::session_start(crate::model::Phase::Initial),
            crate::model::Phase::Initial,
        )
        .unwrap();
        let texts: Vec<&str> = selected.rules.iter().map(|s| s.as_str()).collect();
        assert!(
            texts.iter().any(|t| t.contains("always-rule")),
            "always rule は含まれるはず: {texts:?}"
        );
        assert!(
            !texts.iter().any(|t| t.contains("operation-rule")),
            "operation trigger rule は SessionStart に出ないはず: {texts:?}"
        );
    }

    /// always_limit を変えると SessionStart の件数上限が変わる。
    #[test]
    fn session_start_with_limit_respects_custom_limit() {
        let owox = tempdir();
        std::fs::write(
            owox.join("rules.md"),
            "## Common\n- r1\ntrigger: always\n- r2\ntrigger: always\n- r3\ntrigger: always\n",
        )
        .unwrap();
        // limit=2 で 2 件まで
        let selected = select_delivery_for_phase(
            &owox,
            DeliveryRequest::session_start_with_limit(crate::model::Phase::Initial, 2),
            crate::model::Phase::Initial,
        )
        .unwrap();
        assert_eq!(
            selected.rules.len(),
            2,
            "limit=2 なら 2 件: {:?}",
            selected.rules
        );

        // limit=10 で全件 (3件)
        let selected2 = select_delivery_for_phase(
            &owox,
            DeliveryRequest::session_start_with_limit(crate::model::Phase::Initial, 10),
            crate::model::Phase::Initial,
        )
        .unwrap();
        assert_eq!(
            selected2.rules.len(),
            3,
            "limit=10 なら全 3 件: {:?}",
            selected2.rules
        );
    }
}
