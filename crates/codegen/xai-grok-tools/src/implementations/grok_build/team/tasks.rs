use super::store::{TeamStore, TeamStoreError};
use super::types::{TaskStatus, TeamTask};

#[derive(Debug, thiserror::Error)]
pub enum TaskError {
    #[error(transparent)]
    Store(#[from] TeamStoreError),
    #[error("task not found: {0}")]
    NotFound(String),
    #[error("task {0} is already {1:?}")]
    WrongStatus(String, TaskStatus),
    #[error("task {0} is blocked by unfinished dependencies: {1:?}")]
    Blocked(String, Vec<String>),
    #[error("task {0} is already claimed by {1}")]
    AlreadyClaimed(String, String),
    #[error("unknown member: {0}")]
    UnknownMember(String),
}

pub fn create_task(
    store: &TeamStore,
    team_id: &str,
    id: String,
    title: String,
    depends_on: Vec<String>,
    now_unix_ms: i64,
) -> Result<TeamTask, TaskError> {
    let mut tasks = store.read_tasks(team_id)?;
    let task = TeamTask {
        id,
        title,
        status: TaskStatus::Pending,
        assignee: None,
        depends_on,
        created_unix_ms: now_unix_ms,
    };
    tasks.push(task.clone());
    store.write_tasks(team_id, &tasks)?;
    Ok(task)
}

pub fn claim_task(
    store: &TeamStore,
    team_id: &str,
    task_id: &str,
    claimant: &str,
) -> Result<TeamTask, TaskError> {
    let config = store.load(team_id)?;
    if config.member(claimant).is_none() {
        return Err(TaskError::UnknownMember(claimant.to_string()));
    }
    let mut tasks = store.read_tasks(team_id)?;
    let (idx, unfinished) = {
        let task = tasks
            .iter()
            .find(|t| t.id == task_id)
            .ok_or_else(|| TaskError::NotFound(task_id.to_string()))?;
        if task.status != TaskStatus::Pending {
            return Err(TaskError::WrongStatus(task_id.to_string(), task.status));
        }
        if let Some(owner) = task.assignee.as_ref() {
            return Err(TaskError::AlreadyClaimed(
                task_id.to_string(),
                owner.clone(),
            ));
        }
        let unfinished: Vec<String> = task
            .depends_on
            .iter()
            .filter(|dep| {
                !tasks
                    .iter()
                    .any(|t| t.id == **dep && t.status == TaskStatus::Completed)
            })
            .cloned()
            .collect();
        if !unfinished.is_empty() {
            return Err(TaskError::Blocked(task_id.to_string(), unfinished));
        }
        let idx = tasks
            .iter()
            .position(|t| t.id == task_id)
            .expect("found above");
        (idx, unfinished)
    };
    let _ = unfinished;
    tasks[idx].status = TaskStatus::InProgress;
    tasks[idx].assignee = Some(claimant.to_string());
    let claimed = tasks[idx].clone();
    store.write_tasks(team_id, &tasks)?;
    Ok(claimed)
}

pub fn complete_task(
    store: &TeamStore,
    team_id: &str,
    task_id: &str,
) -> Result<TeamTask, TaskError> {
    let mut tasks = store.read_tasks(team_id)?;
    let idx = tasks
        .iter()
        .position(|t| t.id == task_id)
        .ok_or_else(|| TaskError::NotFound(task_id.to_string()))?;
    if tasks[idx].status == TaskStatus::Completed {
        return Err(TaskError::WrongStatus(
            task_id.to_string(),
            TaskStatus::Completed,
        ));
    }
    tasks[idx].status = TaskStatus::Completed;
    let done = tasks[idx].clone();
    store.write_tasks(team_id, &tasks)?;
    Ok(done)
}
