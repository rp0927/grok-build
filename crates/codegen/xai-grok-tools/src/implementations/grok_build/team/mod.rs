//! Agent-team store and tools.
//!
//! Teammates are top-level pager sessions. These tools persist membership,
//! mailboxes, and tasks. The pager (P2) materializes `spawn_teammate` rows
//! that still have `session_id: None`.

mod mailbox;
mod store;
mod tasks;
mod tools;
mod types;

pub use mailbox::{MailboxError, send_message};
pub use store::{TeamStore, TeamStoreError};
pub use tasks::{TaskError, claim_task, complete_task, create_task};
pub use tools::{
    SEND_MESSAGE_TOOL_NAME, SPAWN_TEAMMATE_TOOL_NAME, TEAM_STATUS_TOOL_NAME,
    TEAM_TASK_CLAIM_TOOL_NAME, TEAM_TASK_COMPLETE_TOOL_NAME, TEAM_TASK_CREATE_TOOL_NAME,
    TEAM_TOOL_NAMES, SendMessageInput, SendMessageOutput, SendMessageTool, SpawnTeammateInput,
    SpawnTeammateOutput, SpawnTeammateTool, TeamStatusInput, TeamStatusOutput, TeamStatusTool,
    TeamTaskClaimInput, TeamTaskClaimOutput, TeamTaskClaimTool, TeamTaskCompleteInput,
    TeamTaskCompleteOutput, TeamTaskCompleteTool, TeamTaskCreateInput, TeamTaskCreateOutput,
    TeamTaskCreateTool, is_team_tool_id,
};
pub use types::{
    MailboxKind, MailboxMessage, MemberRole, TaskStatus, TeamConfig, TeamMember, TeamTask,
    team_id_from_session, valid_member_name,
};

#[cfg(test)]
mod tests;
