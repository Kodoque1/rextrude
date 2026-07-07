pub mod hygiene;
pub mod quality;
pub mod structure;
pub mod sweep;

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Pass,
    Fail,
    Skipped,
}

#[derive(Debug, Clone, Serialize)]
pub struct Violation {
    pub node: Option<String>,
    pub message: String,
}

impl Violation {
    pub fn new(node: impl Into<Option<String>>, message: impl Into<String>) -> Self {
        Self {
            node: node.into(),
            message: message.into(),
        }
    }

    pub fn global(message: impl Into<String>) -> Self {
        Self {
            node: None,
            message: message.into(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct CheckResult {
    pub id: String,
    pub status: Status,
    pub violations: Vec<Violation>,
}

impl CheckResult {
    pub fn pass(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            status: Status::Pass,
            violations: vec![],
        }
    }

    pub fn skipped(id: impl Into<String>, reason: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            status: Status::Skipped,
            violations: vec![Violation::global(reason)],
        }
    }

    pub fn from_violations(id: impl Into<String>, violations: Vec<Violation>) -> Self {
        let status = if violations.is_empty() {
            Status::Pass
        } else {
            Status::Fail
        };
        Self {
            id: id.into(),
            status,
            violations,
        }
    }
}
