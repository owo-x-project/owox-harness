//! owox-core: 正本モデル・読込・型付き生成の中核。
//!
//! 共通判断を core に集め、mcp / setup から同じ中核を呼ぶ。

pub mod agents;
pub mod branch_memory;
pub mod canon;
pub mod canon_detect;
pub mod commands;
pub mod decay;
pub mod envelope;
pub mod experience;
pub mod gate;
pub mod glossary;
pub mod hook;
pub mod irreversible;
pub mod knowledge;
pub mod load;
pub mod markdown;
pub mod model;
pub mod practices;
pub mod profile;
pub mod quality;
pub mod record;
pub mod release;
pub mod requirement;
pub mod review;
pub mod routine;
pub mod secret;
pub mod skill;
pub mod state;
pub mod target;
pub mod targets;
pub mod task;
pub mod tokens;
pub mod usage;
pub mod verify;

pub use agents::{Agents, Role, Sandbox, Variant};
pub use branch_memory::{
    BranchMemory, add_branch_note, get_branch_memory_envelope, list_branch_memories,
};
pub use canon::{
    ProposeInput, apply_pending_canon_change, canon_add, canon_propose,
    propose_practice_from_correction, revert_pending_canon_change,
};
pub use canon_detect::{
    BoundaryDraft, CanonDraft, IrreversibleDraft, LayerDraft, detect_canon_draft,
    render_quality_toml, render_rules_markdown,
};
pub use commands::{Command, command_skills, load_commands};
pub use decay::{
    DecayFinding, run_branch_memory_decay, run_code_decay, run_decay, run_knowledge_decay,
    run_practice_decay, run_practice_redundancy,
};
pub use envelope::{Envelope, Gate, Status};
pub use experience::{export as experience_export, import as experience_import};
pub use gate::{Enforcement, autonomy_enforcement, commit_blocks, compose, phase_enforcement};
pub use glossary::lookup as glossary_lookup;
pub use hook::{
    GateAuthorization, GlossaryInjection, HookDecision, LayerGate, PolicyInjection, StopDecision,
    VerifyOutcome, commit_gate, floor_context, glossary_injection, is_git_commit,
    layer_pre_action_gate, parse_patch_changes, policy_injection, pre_tool_use_decision,
    render_rules_block, render_skills_section, stop_decision,
};
pub use knowledge::{
    Knowledge, KnowledgeInput, KnowledgeStatus, add_knowledge, get_knowledge, list_knowledge,
    list_knowledge_envelope, lookup_knowledge,
};
pub use load::{LoadError, load_canon};
pub use model::{
    Brand, Canon, Context, ContextEntry, ContextLimits, ForbiddenTerm, Glossary, GlossaryEntry,
    HumanGate, Irreversible, ModelTier, Phase, Practice, Practices, Rules, ScopeKind, Settings,
    State, TargetSpec, Targets, VerifyCheck, VerifyConfig,
};
pub use practices::lookup as practice_lookup;
pub use profile::{
    Architecture, Axes, AxisLean, Delivery, DetectSignals, PartialAxes, Prioritization, Profile,
    ProfileDraft, RequirementsShape, builtin_bundle_names, detect_profile, set_profile,
};
pub use quality::{
    Autonomy, Boundary, DecayConfig, Layer, Quality, QualityViolation, RoutineConfig, SizeBudget,
    run_brand, run_quality,
};
pub use record::{
    Decision, DecisionLinks, DecisionStatus, ProposedChange, RecordInput, approve_gate,
    approve_gate_auto, confirm_auto_approval, gate_autonomy, list_auto_pending, list_decisions,
    list_gates, load_decision, mark_gate_consumed, record_decision,
    record_decision_with_authorization, record_decision_with_change, reject_decision,
};
pub use release::{Release, VersionSource};
pub use requirement::{
    AcceptanceCriterion, CreateRequirementInput, CriterionInput, Met, Requirement, RequirementKind,
    RequirementLinks, RequirementStatus, UpdateRequirementInput, add_criterion, create_requirement,
    get_requirement, layer_progress, link_verification, list_requirements,
    list_requirements_envelope, update_requirement,
};
pub use review::{Applicability, Lens, load_lenses, review_lenses_envelope, select_lenses};
pub use routine::{RoutineSuggestion, run_routine_suggestions};
pub use skill::{
    ScriptFile, Skill, SkillStatus, Stage, TestState, list_skills_envelope, load_skills,
    promote_skill, register_skill, registered_skills, remember, run_skill_tests, skill_status,
};
pub use state::set_state;
pub use target::{GeneratedFile, Target, Write, WriteError, find, registry, write_all};
pub use task::{
    CreateTaskInput, Dep, DepKind, ExternalRef, Task, TaskLinks, TaskStatus, UpdateTaskInput,
    add_note, close_task, create_task, drop_task, is_ready, link_task, list_tasks,
    list_tasks_envelope, update_task,
};
pub use verify::{CheckResult, run_checks, run_verify};
