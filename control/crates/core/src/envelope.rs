//! tool 共通返り値 (封筒)。全 tool がこの形で返す。
//!
//! 失敗・人間判断点・完了を一様に扱うため、status / reason / next_actions /
//! decision_ids / data / gate を統一する (`docs/decisions/20260611-MCP設計.md`)。
//!
//! core は Serialize だけ持つ。schemars (JsonSchema) は付けない。
//! JsonSchema は MCP 境界の都合であり、core を MCP へ結合させない。
//! mcp 側で JSON へ直列化し CallToolResult のテキストとして返す
//! (`docs/decisions/20260613-Phase4-tool記録層.md`)。

use serde::Serialize;
use serde_json::Value;

/// tool の結末。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    /// 成功。data に本体。
    Ok,
    /// 人間判断が要る。gate に判断点。AI はこれを人間へ提示し止まる。
    NeedsHuman,
    /// 失敗。reason に原因。
    Failed,
}

/// 人間判断点。needs_human の時だけ付く。
#[derive(Debug, Clone, Serialize)]
pub struct Gate {
    /// 判断点の種類 (completion-judgment / irreversible / brand 等)。
    pub kind: String,
    /// 何についての判断か。
    pub subject: String,
    /// 人間に求めること。
    pub requires: String,
}

/// 全 tool 共通の返り値。
#[derive(Debug, Clone, Serialize)]
pub struct Envelope {
    /// 結末。
    pub status: Status,
    /// なぜそうなったか。
    pub reason: String,
    /// 次の手。
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub next_actions: Vec<String>,
    /// 関連する来歴 ID。
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub decision_ids: Vec<String>,
    /// ok 時の本体 (tool 別)。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
    /// needs_human 時の判断点。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gate: Option<Gate>,
}

impl Envelope {
    /// 成功。本体 data を添える。
    pub fn ok(reason: impl Into<String>, data: Value) -> Envelope {
        Envelope {
            status: Status::Ok,
            reason: reason.into(),
            next_actions: Vec::new(),
            decision_ids: Vec::new(),
            data: Some(data),
            gate: None,
        }
    }

    /// 人間判断が要る。判断点を添え、AI へ提示して止まらせる。
    pub fn needs_human(reason: impl Into<String>, gate: Gate) -> Envelope {
        Envelope {
            status: Status::NeedsHuman,
            reason: reason.into(),
            next_actions: Vec::new(),
            decision_ids: Vec::new(),
            data: None,
            gate: Some(gate),
        }
    }

    /// 失敗。
    pub fn failed(reason: impl Into<String>) -> Envelope {
        Envelope {
            status: Status::Failed,
            reason: reason.into(),
            next_actions: Vec::new(),
            decision_ids: Vec::new(),
            data: None,
            gate: None,
        }
    }

    /// 次の手を添える。
    pub fn with_next_actions(mut self, actions: Vec<String>) -> Envelope {
        self.next_actions = actions;
        self
    }

    /// 関連来歴 ID を添える。
    pub fn with_decision_ids(mut self, ids: Vec<String>) -> Envelope {
        self.decision_ids = ids;
        self
    }

    /// data を後付けする (needs_human でも完了内訳などを返す時)。
    pub fn with_data(mut self, data: Value) -> Envelope {
        self.data = Some(data);
        self
    }
}
