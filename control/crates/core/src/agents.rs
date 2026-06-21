//! agents.toml: オーケストレーション正本。役割 × 変種の2次元定義。
//!
//! CLI 非依存の正本で、役割(基本5)×変種+責務+ティア+sandbox+許可層+文脈を型定義する。
//! 組込み既定 (5役割+2変種) を持ち、agents.toml で部分上書き・変種追記ができる。
//! (`docs/decisions/20260620-オーケストレーション具体化.md`)

use serde::Deserialize;
use std::collections::BTreeMap;

use crate::quality::Autonomy;

/// subagent のファイル操作権限域。Codex の sandbox_mode 値と一致する。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sandbox {
    /// 読取専用。
    ReadOnly,
    /// ワークスペース書込可。
    WorkspaceWrite,
}

impl Sandbox {
    /// Codex sandbox_mode 値へ変換する。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ReadOnly => "read-only",
            Self::WorkspaceWrite => "workspace-write",
        }
    }

    fn parse(value: &str) -> Result<Self, String> {
        match value.trim() {
            "read-only" => Ok(Self::ReadOnly),
            "workspace-write" => Ok(Self::WorkspaceWrite),
            other => Err(format!(
                "sandbox は read-only / workspace-write のみ: {other}"
            )),
        }
    }
}

/// 役割 1 件。何をする存在かを定義する。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Role {
    /// 役割識別子 (基本5: investigate / plan / implement / review / verify)。
    pub id: String,
    /// 責務の英語記述。spawn 時に指示文として使う。
    pub responsibility: String,
    /// モデルティア (fast / balanced / strong / reasoning)。
    pub tier: String,
    /// ファイル操作権限域。
    pub sandbox: Sandbox,
    /// 書込を許可する自律度層。実装役割のみ [Supervised, Free]、他は空。
    pub writes_layers: Vec<Autonomy>,
    /// spawn 時に渡す文脈の説明。
    pub context: String,
}

/// 変種 1 件。役割を専門化する分岐軸。spawn 時に prompt 上書き (+任意 tier 上書き) で注入する。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Variant {
    /// 変種識別子。
    pub id: String,
    /// 適用する役割の id。
    pub applies_to: String,
    /// spawn 時に注入するプロンプト差分。
    pub prompt: String,
    /// ティア上書き。None なら役割の tier をそのまま使う。
    pub tier_override: Option<String>,
}

/// エージェント定義の全体。roles と variants の組み合わせで役割 × 変種の2次元を表す。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Agents {
    pub roles: Vec<Role>,
    pub variants: Vec<Variant>,
}

impl Default for Agents {
    fn default() -> Self {
        Agents {
            roles: builtin_roles(),
            variants: builtin_variants(),
        }
    }
}

impl Agents {
    /// agents.toml を読み組込み既定へ上書きを重ねる。
    ///
    /// roles は組込み5の同 id にのみ部分上書きできる。未知 id はエラー (役割は5固定)。
    /// variants はプロジェクト定義を組込みへ追記し、同 id は上書きする。
    /// deny_unknown_fields・未知の tier や sandbox 値は解釈エラーになる。
    pub fn from_toml(text: &str) -> Result<Agents, String> {
        let raw: AgentsRaw = toml::from_str(text).map_err(|e| e.to_string())?;
        let mut roles = builtin_roles();

        for (id, role_raw) in raw.roles {
            let slot = roles
                .iter_mut()
                .find(|r| r.id == id)
                .ok_or_else(|| format!("未知の役割 id: {id} (基本5のみ: investigate / plan / implement / review / verify)"))?;

            if let Some(v) = role_raw.tier {
                slot.tier = v;
            }
            if let Some(v) = role_raw.sandbox {
                slot.sandbox = Sandbox::parse(&v)?;
            }
            if let Some(v) = role_raw.responsibility
                && !v.trim().is_empty()
            {
                slot.responsibility = v;
            }
            if let Some(v) = role_raw.writes_layers {
                slot.writes_layers = v
                    .iter()
                    .map(|s| Autonomy::parse(s))
                    .collect::<Result<Vec<_>, _>>()?;
            }
            if let Some(v) = role_raw.context
                && !v.trim().is_empty()
            {
                slot.context = v;
            }
        }

        let mut variants = builtin_variants();
        for variant_raw in raw.variants {
            let v = Variant {
                id: variant_raw.id.clone(),
                applies_to: variant_raw.applies_to,
                prompt: variant_raw.prompt,
                tier_override: variant_raw.tier_override.filter(|s| !s.trim().is_empty()),
            };
            upsert_variant(&mut variants, v);
        }

        Ok(Agents { roles, variants })
    }
}

/// 同 id があれば差し替え、無ければ追加する。
fn upsert_variant(variants: &mut Vec<Variant>, v: Variant) {
    if let Some(slot) = variants.iter_mut().find(|x| x.id == v.id) {
        *slot = v;
    } else {
        variants.push(v);
    }
}

/// 組込みの5役割既定。
fn builtin_roles() -> Vec<Role> {
    vec![
        Role {
            id: "investigate".to_string(),
            responsibility: "Investigate how something works — code, an API, a library, or a protocol — and report findings. Do not change code.".to_string(),
            tier: "fast".to_string(),
            sandbox: Sandbox::ReadOnly,
            writes_layers: vec![],
            context: "the context map for the area in question".to_string(),
        },
        Role {
            id: "plan".to_string(),
            responsibility: "Turn a goal into a plan: propose requirements, tasks, and decisions, recording them through owox tools. Do not change code.".to_string(),
            tier: "strong".to_string(),
            sandbox: Sandbox::ReadOnly,
            writes_layers: vec![],
            context: "the goal and the relevant requirements and decisions".to_string(),
        },
        Role {
            id: "implement".to_string(),
            responsibility: "Implement a scoped piece of work by editing code. Do not commit, and do not touch guarded assets such as contracts, rules, brand, or glossary.".to_string(),
            tier: "balanced".to_string(),
            sandbox: Sandbox::WorkspaceWrite,
            writes_layers: vec![Autonomy::Supervised, Autonomy::Free],
            context: "the task and the files it touches".to_string(),
        },
        Role {
            id: "review".to_string(),
            responsibility: "Review a change without modifying it. Take in verify.run first, then review through the applicable perspectives, confirming and adversarially re-checking each finding.".to_string(),
            tier: "strong".to_string(),
            sandbox: Sandbox::ReadOnly,
            writes_layers: vec![],
            context: "the diff and the applicable review lenses".to_string(),
        },
        Role {
            id: "verify".to_string(),
            responsibility: "Run the project's verification checks and report what passed and what did not. Do not change code to make checks pass.".to_string(),
            tier: "strong".to_string(),
            sandbox: Sandbox::WorkspaceWrite,
            writes_layers: vec![],
            context: "the change and the configured checks".to_string(),
        },
    ]
}

/// 組込みの2変種既定。
fn builtin_variants() -> Vec<Variant> {
    vec![
        Variant {
            id: "adversarial".to_string(),
            applies_to: "review".to_string(),
            prompt: "Try to refute each finding. Default to rejecting a finding unless you can prove it is real.".to_string(),
            tier_override: Some("reasoning".to_string()),
        },
        Variant {
            id: "gardener".to_string(),
            applies_to: "review".to_string(),
            prompt: "Apply the pruning perspective: find dead code, duplication, and scaffolding to remove, each routed through the deletion policy and verification before removing anything.".to_string(),
            tier_override: None,
        },
    ]
}

// --- TOML 読込用の素の型 ---

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct AgentsRaw {
    #[serde(default)]
    roles: BTreeMap<String, RoleRaw>,
    #[serde(default)]
    variants: Vec<VariantRaw>,
}

/// 役割の部分上書き。全フィールドが任意 (指定された軸だけ重ねる)。
#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct RoleRaw {
    responsibility: Option<String>,
    tier: Option<String>,
    sandbox: Option<String>,
    writes_layers: Option<Vec<String>>,
    context: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct VariantRaw {
    id: String,
    applies_to: String,
    prompt: String,
    tier_override: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_has_five_roles_and_two_variants() {
        let a = Agents::default();
        assert_eq!(a.roles.len(), 5);
        assert_eq!(a.variants.len(), 2);
        let ids: Vec<_> = a.roles.iter().map(|r| r.id.as_str()).collect();
        assert!(ids.contains(&"investigate"));
        assert!(ids.contains(&"plan"));
        assert!(ids.contains(&"implement"));
        assert!(ids.contains(&"review"));
        assert!(ids.contains(&"verify"));
    }

    #[test]
    fn empty_toml_yields_defaults() {
        let a = Agents::from_toml("").unwrap();
        assert_eq!(a, Agents::default());
    }

    #[test]
    fn implement_has_supervised_and_free_layers() {
        let a = Agents::default();
        let imp = a.roles.iter().find(|r| r.id == "implement").unwrap();
        assert_eq!(
            imp.writes_layers,
            vec![Autonomy::Supervised, Autonomy::Free]
        );
        assert_eq!(imp.sandbox, Sandbox::WorkspaceWrite);
    }

    #[test]
    fn investigate_has_empty_writes_layers() {
        let a = Agents::default();
        let inv = a.roles.iter().find(|r| r.id == "investigate").unwrap();
        assert!(inv.writes_layers.is_empty());
        assert_eq!(inv.sandbox, Sandbox::ReadOnly);
    }

    #[test]
    fn partial_role_override_changes_only_tier() {
        let toml = "[roles.implement]\ntier = \"strong\"\n";
        let a = Agents::from_toml(toml).unwrap();
        let imp = a.roles.iter().find(|r| r.id == "implement").unwrap();
        assert_eq!(imp.tier, "strong");
        // 他のフィールドは既定のまま。
        assert_eq!(imp.sandbox, Sandbox::WorkspaceWrite);
        assert_eq!(
            imp.writes_layers,
            vec![Autonomy::Supervised, Autonomy::Free]
        );
    }

    #[test]
    fn unknown_role_id_is_rejected() {
        let toml = "[roles.executor]\ntier = \"fast\"\n";
        let err = Agents::from_toml(toml).unwrap_err();
        assert!(err.contains("executor"), "{err}");
        assert!(err.contains("investigate"), "{err}");
    }

    #[test]
    fn variant_appended_to_defaults() {
        let toml = "[[variants]]\nid = \"security-audit\"\napplies_to = \"review\"\nprompt = \"Focus on security issues.\"\n";
        let a = Agents::from_toml(toml).unwrap();
        assert_eq!(a.variants.len(), 3);
        let v = a
            .variants
            .iter()
            .find(|v| v.id == "security-audit")
            .unwrap();
        assert_eq!(v.applies_to, "review");
        assert_eq!(v.tier_override, None);
    }

    #[test]
    fn variant_same_id_overrides_builtin() {
        let toml = "[[variants]]\nid = \"adversarial\"\napplies_to = \"review\"\nprompt = \"Custom refute.\"\ntier_override = \"strong\"\n";
        let a = Agents::from_toml(toml).unwrap();
        // 同 id は上書き → 合計2件のまま。
        assert_eq!(a.variants.len(), 2);
        let v = a.variants.iter().find(|v| v.id == "adversarial").unwrap();
        assert_eq!(v.prompt, "Custom refute.");
        assert_eq!(v.tier_override.as_deref(), Some("strong"));
    }

    #[test]
    fn unknown_field_in_role_is_rejected() {
        let toml = "[roles.investigate]\nbogus = \"x\"\n";
        let err = Agents::from_toml(toml).unwrap_err();
        assert!(err.contains("bogus") || err.contains("unknown"), "{err}");
    }

    #[test]
    fn unknown_field_at_top_level_is_rejected() {
        let toml = "[bogus]\nx = 1\n";
        let err = Agents::from_toml(toml).unwrap_err();
        assert!(err.contains("bogus") || err.contains("unknown"), "{err}");
    }

    #[test]
    fn unknown_sandbox_value_is_rejected() {
        let toml = "[roles.investigate]\nsandbox = \"network-write\"\n";
        let err = Agents::from_toml(toml).unwrap_err();
        assert!(
            err.contains("read-only") || err.contains("workspace-write"),
            "{err}"
        );
    }

    #[test]
    fn adversarial_variant_has_reasoning_tier() {
        let a = Agents::default();
        let v = a.variants.iter().find(|v| v.id == "adversarial").unwrap();
        assert_eq!(v.tier_override.as_deref(), Some("reasoning"));
    }

    #[test]
    fn gardener_variant_has_no_tier_override() {
        let a = Agents::default();
        let v = a.variants.iter().find(|v| v.id == "gardener").unwrap();
        assert_eq!(v.tier_override, None);
    }

    #[test]
    fn sandbox_as_str_matches_codex_values() {
        assert_eq!(Sandbox::ReadOnly.as_str(), "read-only");
        assert_eq!(Sandbox::WorkspaceWrite.as_str(), "workspace-write");
    }
}
