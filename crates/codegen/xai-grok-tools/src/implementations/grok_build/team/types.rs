use serde::{Deserialize, Serialize};

/// `session-` plus the first eight hex characters of the lead session id.
pub fn team_id_from_session(session_id: &str) -> String {
    let compact: String = session_id
        .chars()
        .filter(|c| c.is_ascii_hexdigit())
        .take(8)
        .collect();
    let slug = if compact.is_empty() {
        "unknown".to_string()
    } else {
        compact.to_ascii_lowercase()
    };
    format!("session-{slug}")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum MemberRole {
    TeamLead,
    Teammate,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TeamMember {
    pub name: String,
    pub role: MemberRole,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    /// Prompt the pager should send after it materializes this member (P2).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spawn_prompt: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TeamConfig {
    pub team_id: String,
    pub lead_session_id: String,
    pub members: Vec<TeamMember>,
    pub created_unix_ms: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MailboxKind {
    Text,
    PlanApproval,
    Shutdown,
    TaskUpdate,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MailboxMessage {
    pub id: String,
    pub from: String,
    pub to: String,
    pub kind: MailboxKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subject: Option<String>,
    pub body: String,
    pub created_unix_ms: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Pending,
    InProgress,
    Completed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TeamTask {
    pub id: String,
    pub title: String,
    pub status: TaskStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assignee: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub depends_on: Vec<String>,
    pub created_unix_ms: i64,
}

impl TeamConfig {
    pub fn member(&self, name: &str) -> Option<&TeamMember> {
        self.members.iter().find(|m| m.name == name)
    }

    pub fn lead(&self) -> Option<&TeamMember> {
        self.members.iter().find(|m| m.role == MemberRole::TeamLead)
    }

    pub fn member_for_session(&self, session_id: &str) -> Option<&TeamMember> {
        self.members
            .iter()
            .find(|m| m.session_id.as_deref() == Some(session_id))
            .or_else(|| {
                if self.lead_session_id == session_id {
                    self.lead()
                } else {
                    None
                }
            })
    }
}

/// `[a-z][a-z0-9_-]{0,31}` — same shape as Herdr agent names.
pub fn valid_member_name(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(c) if c.is_ascii_lowercase() => {
            name.len() <= 32
                && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
        }
        _ => false,
    }
}
