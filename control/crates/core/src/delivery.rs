//! rules / practices の配送判定。
//!
//! 正本本文は既存 model が読むが、配送用 trigger 属性はここで軽く読む。
//! 永続 model を広げず、必要な時だけ rules / practices を届けるための層。

use std::path::Path;

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
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DeliverySelection {
    pub rules: Vec<String>,
    pub practices: Vec<String>,
}

pub struct DeliveryRequest<'a> {
    pub operations: &'a [DeliveryOperation],
    pub paths: &'a [String],
    pub include_always: bool,
    pub include_path: bool,
    pub include_operation: bool,
    pub always_limit: usize,
}

impl<'a> DeliveryRequest<'a> {
    pub fn session_start() -> Self {
        Self {
            operations: &[],
            paths: &[],
            include_always: true,
            include_path: false,
            include_operation: false,
            always_limit: 3,
        }
    }

    pub fn for_paths(paths: &'a [String]) -> Self {
        Self {
            operations: &[],
            paths,
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
    let mut all = parse_rules(&owox_dir.join("rules.md"))?;
    all.extend(parse_practices(&owox_dir.join("practices.md"))?);

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
    let mut matched = false;
    if req.include_always && entry.triggers.contains(&TriggerKind::Always) {
        matched = true;
    }
    if req.include_operation
        && entry.triggers.contains(&TriggerKind::Operation)
        && matches_operations(entry, req.operations)
    {
        matched = true;
    }
    if req.include_path
        && entry.triggers.contains(&TriggerKind::Path)
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

fn parse_rules(path: &Path) -> Result<Vec<DeliveryEntry>, String> {
    parse_entries(path, DeliverySurface::Rules)
}

fn parse_practices(path: &Path) -> Result<Vec<DeliveryEntry>, String> {
    parse_entries(path, DeliverySurface::Practices)
}

fn parse_entries(path: &Path, surface: DeliverySurface) -> Result<Vec<DeliveryEntry>, String> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(format!("{} を読めない: {err}", path.display())),
    };

    let mut section = String::new();
    let mut order = 0usize;
    let mut current: Option<RawEntry> = None;
    let mut out = Vec::new();

    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with("# ") {
            continue;
        }
        if line.starts_with("## ") && !line.starts_with("### ") {
            flush_entry(surface, &mut current, &mut out, &mut order)?;
            section = line["## ".len()..].trim().to_string();
            continue;
        }
        if let Some(item) = line.strip_prefix("- ") {
            flush_entry(surface, &mut current, &mut out, &mut order)?;
            current = Some(RawEntry {
                section: section.clone(),
                text: item.trim().to_string(),
                trigger_names: Vec::new(),
                operations: Vec::new(),
                paths: Vec::new(),
            });
            continue;
        }
        let Some(entry) = current.as_mut() else {
            continue;
        };
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let values: Vec<String> = value
            .split(',')
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(str::to_string)
            .collect();
        match key.trim() {
            "trigger" => entry.trigger_names.extend(values),
            "operation" => entry.operations.extend(values),
            "path" => entry.paths.extend(values),
            _ => {}
        }
    }
    flush_entry(surface, &mut current, &mut out, &mut order)?;
    Ok(out)
}

struct RawEntry {
    section: String,
    text: String,
    trigger_names: Vec<String>,
    operations: Vec<String>,
    paths: Vec<String>,
}

fn flush_entry(
    surface: DeliverySurface,
    current: &mut Option<RawEntry>,
    out: &mut Vec<DeliveryEntry>,
    order: &mut usize,
) -> Result<(), String> {
    let Some(raw) = current.take() else {
        return Ok(());
    };
    let triggers = resolve_triggers(surface, &raw)?;
    let operations = raw
        .operations
        .iter()
        .map(|v| DeliveryOperation::parse(v))
        .collect::<Result<Vec<_>, _>>()?;
    let text = if raw.section.is_empty() {
        raw.text
    } else {
        format!("{}: {}", raw.section, raw.text)
    };
    out.push(DeliveryEntry {
        surface,
        text,
        order: *order,
        triggers,
        operations,
        paths: raw.paths,
    });
    *order += 1;
    out.sort_by_key(|e| e.order);
    Ok(())
}

fn resolve_triggers(surface: DeliverySurface, raw: &RawEntry) -> Result<Vec<TriggerKind>, String> {
    let mut out = Vec::new();
    if !raw.trigger_names.is_empty() {
        for name in &raw.trigger_names {
            let trigger = match name.as_str() {
                "always" => TriggerKind::Always,
                "path" => TriggerKind::Path,
                "operation" => TriggerKind::Operation,
                other => return Err(format!("未知の trigger: {other}")),
            };
            if !out.contains(&trigger) {
                out.push(trigger);
            }
        }
        return Ok(out);
    }
    if !raw.operations.is_empty() {
        out.push(TriggerKind::Operation);
    }
    if !raw.paths.is_empty() {
        out.push(TriggerKind::Path);
    }
    if out.is_empty() {
        out.push(match surface {
            DeliverySurface::Rules => TriggerKind::Operation,
            DeliverySurface::Practices => TriggerKind::Path,
        });
    }
    Ok(out)
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
        let selected = select_delivery(&owox, DeliveryRequest::session_start()).unwrap();
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
}
