//! Agent-team store: membership, mailbox, and shared tasks.
//!
//! Teammates are top-level pager sessions. This module does not spawn them;
//! it only persists the coordination state those sessions share. See
//! `docs/design/agent-teams.md`.

mod mailbox;
mod store;
mod tasks;
mod types;

pub use mailbox::{MailboxError, send_message};
pub use store::{TeamStore, TeamStoreError};
pub use tasks::{TaskError, claim_task, complete_task, create_task};
pub use types::{
    MailboxKind, MailboxMessage, MemberRole, TaskStatus, TeamConfig, TeamMember, TeamTask,
    team_id_from_session,
};

#[cfg(test)]
mod tests;
