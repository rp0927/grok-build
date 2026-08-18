use super::store::{TeamStore, TeamStoreError};
use super::types::{MailboxKind, MailboxMessage};

#[derive(Debug, thiserror::Error)]
pub enum MailboxError {
    #[error(transparent)]
    Store(#[from] TeamStoreError),
    #[error("sender is not a team member: {0}")]
    UnknownSender(String),
    #[error("recipient is not a team member: {0}")]
    UnknownRecipient(String),
    #[error("cannot send a mailbox message to self")]
    SelfSend,
}

/// Append one message to `to`'s inbox. The write is atomic. Unknown members
/// fail before the file is touched.
pub fn send_message(
    store: &TeamStore,
    team_id: &str,
    from: &str,
    to: &str,
    kind: MailboxKind,
    subject: Option<String>,
    body: String,
    now_unix_ms: i64,
    id: String,
) -> Result<MailboxMessage, MailboxError> {
    if from == to {
        return Err(MailboxError::SelfSend);
    }
    let config = store.load(team_id)?;
    if config.member(from).is_none() {
        return Err(MailboxError::UnknownSender(from.to_string()));
    }
    if config.member(to).is_none() {
        return Err(MailboxError::UnknownRecipient(to.to_string()));
    }
    let message = MailboxMessage {
        id,
        from: from.to_string(),
        to: to.to_string(),
        kind,
        subject,
        body,
        created_unix_ms: now_unix_ms,
    };
    let mut inbox = store.read_inbox(team_id, to)?;
    inbox.push(message.clone());
    store.write_inbox(team_id, to, &inbox)?;
    Ok(message)
}
