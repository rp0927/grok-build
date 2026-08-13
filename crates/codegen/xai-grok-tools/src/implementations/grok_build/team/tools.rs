//! Model-facing team tools. Gated by `GROK_EXPERIMENTAL_AGENT_TEAMS` in
//! `AgentBuilder`; never listed for `PromptAudience::Subagent`.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::mailbox::send_message;
use super::store::TeamStore;
use super::tasks::{claim_task, complete_task, create_task};
use super::types::{MailboxKind, MemberRole, TeamConfig, valid_member_name};
use crate::types::requirements::{Expr, ToolRequirement};
use crate::types::tool::{ToolKind, ToolNamespace};

pub const SPAWN_TEAMMATE_TOOL_NAME: &str = "spawn_teammate";
pub const SEND_MESSAGE_TOOL_NAME: &str = "send_message";
pub const TEAM_TASK_CREATE_TOOL_NAME: &str = "team_task_create";
pub const TEAM_TASK_CLAIM_TOOL_NAME: &str = "team_task_claim";
pub const TEAM_TASK_COMPLETE_TOOL_NAME: &str = "team_task_complete";
pub const TEAM_STATUS_TOOL_NAME: &str = "team_status";

pub const TEAM_TOOL_NAMES: &[&str] = &[
    SPAWN_TEAMMATE_TOOL_NAME,
    SEND_MESSAGE_TOOL_NAME,
    TEAM_TASK_CREATE_TOOL_NAME,
    TEAM_TASK_CLAIM_TOOL_NAME,
    TEAM_TASK_COMPLETE_TOOL_NAME,
    TEAM_STATUS_TOOL_NAME,
];

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn session_id(ctx: &xai_tool_runtime::ToolCallContext) -> Result<String, xai_tool_runtime::ToolError> {
    ctx.get::<xai_tool_runtime::SessionContext>()
        .map(|s| s.0.clone())
        .ok_or_else(|| {
            xai_tool_runtime::ToolError::custom("missing_session", "SessionContext is required")
        })
}

fn store() -> TeamStore {
    TeamStore::new(xai_grok_config::grok_home())
}

fn require_member<'a>(
    team: &'a TeamConfig,
    session_id: &str,
) -> Result<&'a super::types::TeamMember, xai_tool_runtime::ToolError> {
    team.member_for_session(session_id).ok_or_else(|| {
        xai_tool_runtime::ToolError::invalid_arguments(
            "this session is not a member of the team",
        )
    })
}

fn require_lead(team: &TeamConfig, session_id: &str) -> Result<(), xai_tool_runtime::ToolError> {
    let member = require_member(team, session_id)?;
    if member.role != MemberRole::TeamLead {
        return Err(xai_tool_runtime::ToolError::invalid_arguments(
            "only the team lead can spawn teammates",
        ));
    }
    Ok(())
}

fn team_error(err: impl std::fmt::Display) -> xai_tool_runtime::ToolError {
    xai_tool_runtime::ToolError::invalid_arguments(err.to_string())
}

// --- spawn_teammate ---

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SpawnTeammateInput {
    #[schemars(description = "Short unique name: [a-z][a-z0-9_-]{0,31}, e.g. reviewer")]
    pub name: String,
    #[schemars(description = "Prompt the teammate should receive when its session starts")]
    pub prompt: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SpawnTeammateOutput {
    pub team_id: String,
    pub name: String,
    pub spawn_pending: bool,
    pub message: String,
}

impl xai_tool_runtime::ToolOutput for SpawnTeammateOutput {}

#[derive(Debug, Default)]
pub struct SpawnTeammateTool;

impl crate::types::tool_metadata::ToolMetadata for SpawnTeammateTool {
    fn kind(&self) -> ToolKind {
        ToolKind::Other
    }
    fn tool_namespace(&self) -> ToolNamespace {
        ToolNamespace::GrokBuild
    }
    fn description_template(&self) -> &str {
        "Spawn a named teammate for this agent team. The teammate is a full top-level session, not a subagent. Only the lead may call this. The pager creates the session after this call; do not also call spawn_subagent."
    }
    fn requires_expr(&self) -> Expr<ToolRequirement> {
        Expr::True
    }
}

impl xai_tool_runtime::Tool for SpawnTeammateTool {
    type Args = SpawnTeammateInput;
    type Output = SpawnTeammateOutput;

    fn id(&self) -> xai_tool_protocol::ToolId {
        xai_tool_protocol::ToolId::new(SPAWN_TEAMMATE_TOOL_NAME).expect("valid tool id")
    }

    fn description(
        &self,
        _ctx: &xai_tool_runtime::ListToolsContext,
    ) -> xai_tool_types::ToolDescription {
        xai_tool_types::ToolDescription::new(
            SPAWN_TEAMMATE_TOOL_NAME,
            crate::types::tool_metadata::ToolMetadata::sanitized_description_template(self),
        )
    }

    fn capabilities(&self) -> xai_tool_protocol::ToolCapabilities {
        xai_tool_protocol::ToolCapabilities {
            is_read_only: false,
            tool_scope: Some(xai_tool_protocol::ToolScope::Write),
            ..Default::default()
        }
    }

    #[tracing::instrument(name = "tool.spawn_teammate", skip_all)]
    async fn run(
        &self,
        ctx: xai_tool_runtime::ToolCallContext,
        input: SpawnTeammateInput,
    ) -> Result<SpawnTeammateOutput, xai_tool_runtime::ToolError> {
        if !valid_member_name(&input.name) {
            return Err(xai_tool_runtime::ToolError::invalid_arguments(
                "name must match [a-z][a-z0-9_-]{0,31}",
            ));
        }
        if input.prompt.trim().is_empty() {
            return Err(xai_tool_runtime::ToolError::invalid_arguments(
                "prompt must not be empty",
            ));
        }
        let sid = session_id(&ctx)?;
        let store = store();
        let team = match store.find_by_session(&sid).map_err(team_error)? {
            Some(existing) => {
                require_lead(&existing, &sid)?;
                existing
            }
            None => store
                .create_for_lead(&sid, "lead", now_ms())
                .map_err(team_error)?,
        };
        if input.name == "lead" {
            return Err(xai_tool_runtime::ToolError::invalid_arguments(
                "name 'lead' is reserved",
            ));
        }
        store
            .add_member(
                &team.team_id,
                &input.name,
                None,
                Some(input.prompt),
            )
            .map_err(team_error)?;
        Ok(SpawnTeammateOutput {
            team_id: team.team_id,
            name: input.name,
            spawn_pending: true,
            message: "Teammate recorded. The pager will start a top-level session (not a subagent).".into(),
        })
    }
}

// --- send_message ---

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SendMessageInput {
    #[schemars(description = "Recipient member name")]
    pub to: String,
    pub body: String,
    #[serde(default)]
    pub subject: Option<String>,
    #[serde(default)]
    #[schemars(description = "text | plan_approval | shutdown | task_update")]
    pub kind: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SendMessageOutput {
    pub id: String,
    pub to: String,
}

impl xai_tool_runtime::ToolOutput for SendMessageOutput {}

#[derive(Debug, Default)]
pub struct SendMessageTool;

impl crate::types::tool_metadata::ToolMetadata for SendMessageTool {
    fn kind(&self) -> ToolKind {
        ToolKind::Other
    }
    fn tool_namespace(&self) -> ToolNamespace {
        ToolNamespace::GrokBuild
    }
    fn description_template(&self) -> &str {
        "Send a mailbox message to another teammate by name. The recipient is a team member, not a subagent. Use this instead of summarizing for the lead to relay."
    }
    fn requires_expr(&self) -> Expr<ToolRequirement> {
        Expr::True
    }
}

impl xai_tool_runtime::Tool for SendMessageTool {
    type Args = SendMessageInput;
    type Output = SendMessageOutput;

    fn id(&self) -> xai_tool_protocol::ToolId {
        xai_tool_protocol::ToolId::new(SEND_MESSAGE_TOOL_NAME).expect("valid tool id")
    }

    fn description(
        &self,
        _ctx: &xai_tool_runtime::ListToolsContext,
    ) -> xai_tool_types::ToolDescription {
        xai_tool_types::ToolDescription::new(
            SEND_MESSAGE_TOOL_NAME,
            crate::types::tool_metadata::ToolMetadata::sanitized_description_template(self),
        )
    }

    fn capabilities(&self) -> xai_tool_protocol::ToolCapabilities {
        xai_tool_protocol::ToolCapabilities {
            is_read_only: false,
            tool_scope: Some(xai_tool_protocol::ToolScope::Write),
            ..Default::default()
        }
    }

    #[tracing::instrument(name = "tool.send_message", skip_all)]
    async fn run(
        &self,
        ctx: xai_tool_runtime::ToolCallContext,
        input: SendMessageInput,
    ) -> Result<SendMessageOutput, xai_tool_runtime::ToolError> {
        if input.body.trim().is_empty() {
            return Err(xai_tool_runtime::ToolError::invalid_arguments(
                "body must not be empty",
            ));
        }
        let sid = session_id(&ctx)?;
        let store = store();
        let team = store
            .find_by_session(&sid)
            .map_err(team_error)?
            .ok_or_else(|| {
                xai_tool_runtime::ToolError::invalid_arguments("this session is not on a team")
            })?;
        let from = require_member(&team, &sid)?;
        let kind = match input.kind.as_deref() {
            None | Some("text") => MailboxKind::Text,
            Some("plan_approval") => MailboxKind::PlanApproval,
            Some("shutdown") => MailboxKind::Shutdown,
            Some("task_update") => MailboxKind::TaskUpdate,
            Some(other) => {
                return Err(xai_tool_runtime::ToolError::invalid_arguments(format!(
                    "unknown kind: {other}"
                )));
            }
        };
        let msg = send_message(
            &store,
            &team.team_id,
            &from.name,
            &input.to,
            kind,
            input.subject,
            input.body,
            now_ms(),
            format!("m-{}", ctx.call_id.as_str()),
        )
        .map_err(team_error)?;
        Ok(SendMessageOutput {
            id: msg.id,
            to: msg.to,
        })
    }
}

// --- team_task_create ---

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TeamTaskCreateInput {
    pub title: String,
    #[serde(default)]
    pub depends_on: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TeamTaskCreateOutput {
    pub id: String,
    pub title: String,
}

impl xai_tool_runtime::ToolOutput for TeamTaskCreateOutput {}

#[derive(Debug, Default)]
pub struct TeamTaskCreateTool;

impl crate::types::tool_metadata::ToolMetadata for TeamTaskCreateTool {
    fn kind(&self) -> ToolKind {
        ToolKind::Other
    }
    fn tool_namespace(&self) -> ToolNamespace {
        ToolNamespace::GrokBuild
    }
    fn description_template(&self) -> &str {
        "Add a pending task to the team's shared list. Other members can claim it with team_task_claim."
    }
    fn requires_expr(&self) -> Expr<ToolRequirement> {
        Expr::True
    }
}

impl xai_tool_runtime::Tool for TeamTaskCreateTool {
    type Args = TeamTaskCreateInput;
    type Output = TeamTaskCreateOutput;

    fn id(&self) -> xai_tool_protocol::ToolId {
        xai_tool_protocol::ToolId::new(TEAM_TASK_CREATE_TOOL_NAME).expect("valid tool id")
    }

    fn description(
        &self,
        _ctx: &xai_tool_runtime::ListToolsContext,
    ) -> xai_tool_types::ToolDescription {
        xai_tool_types::ToolDescription::new(
            TEAM_TASK_CREATE_TOOL_NAME,
            crate::types::tool_metadata::ToolMetadata::sanitized_description_template(self),
        )
    }

    fn capabilities(&self) -> xai_tool_protocol::ToolCapabilities {
        xai_tool_protocol::ToolCapabilities {
            is_read_only: false,
            tool_scope: Some(xai_tool_protocol::ToolScope::Write),
            ..Default::default()
        }
    }

    #[tracing::instrument(name = "tool.team_task_create", skip_all)]
    async fn run(
        &self,
        ctx: xai_tool_runtime::ToolCallContext,
        input: TeamTaskCreateInput,
    ) -> Result<TeamTaskCreateOutput, xai_tool_runtime::ToolError> {
        if input.title.trim().is_empty() {
            return Err(xai_tool_runtime::ToolError::invalid_arguments(
                "title must not be empty",
            ));
        }
        let sid = session_id(&ctx)?;
        let store = store();
        let team = store
            .find_by_session(&sid)
            .map_err(team_error)?
            .ok_or_else(|| {
                xai_tool_runtime::ToolError::invalid_arguments("this session is not on a team")
            })?;
        require_member(&team, &sid)?;
        let task = create_task(
            &store,
            &team.team_id,
            format!("t-{}", ctx.call_id.as_str()),
            input.title,
            input.depends_on,
            now_ms(),
        )
        .map_err(team_error)?;
        Ok(TeamTaskCreateOutput {
            id: task.id,
            title: task.title,
        })
    }
}

// --- team_task_claim ---

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TeamTaskClaimInput {
    pub task_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TeamTaskClaimOutput {
    pub id: String,
    pub assignee: String,
}

impl xai_tool_runtime::ToolOutput for TeamTaskClaimOutput {}

#[derive(Debug, Default)]
pub struct TeamTaskClaimTool;

impl crate::types::tool_metadata::ToolMetadata for TeamTaskClaimTool {
    fn kind(&self) -> ToolKind {
        ToolKind::Other
    }
    fn tool_namespace(&self) -> ToolNamespace {
        ToolNamespace::GrokBuild
    }
    fn description_template(&self) -> &str {
        "Claim a pending unblocked team task. Fails if another member already claimed it or dependencies are unfinished."
    }
    fn requires_expr(&self) -> Expr<ToolRequirement> {
        Expr::True
    }
}

impl xai_tool_runtime::Tool for TeamTaskClaimTool {
    type Args = TeamTaskClaimInput;
    type Output = TeamTaskClaimOutput;

    fn id(&self) -> xai_tool_protocol::ToolId {
        xai_tool_protocol::ToolId::new(TEAM_TASK_CLAIM_TOOL_NAME).expect("valid tool id")
    }

    fn description(
        &self,
        _ctx: &xai_tool_runtime::ListToolsContext,
    ) -> xai_tool_types::ToolDescription {
        xai_tool_types::ToolDescription::new(
            TEAM_TASK_CLAIM_TOOL_NAME,
            crate::types::tool_metadata::ToolMetadata::sanitized_description_template(self),
        )
    }

    fn capabilities(&self) -> xai_tool_protocol::ToolCapabilities {
        xai_tool_protocol::ToolCapabilities {
            is_read_only: false,
            tool_scope: Some(xai_tool_protocol::ToolScope::Write),
            ..Default::default()
        }
    }

    #[tracing::instrument(name = "tool.team_task_claim", skip_all)]
    async fn run(
        &self,
        ctx: xai_tool_runtime::ToolCallContext,
        input: TeamTaskClaimInput,
    ) -> Result<TeamTaskClaimOutput, xai_tool_runtime::ToolError> {
        let sid = session_id(&ctx)?;
        let store = store();
        let team = store
            .find_by_session(&sid)
            .map_err(team_error)?
            .ok_or_else(|| {
                xai_tool_runtime::ToolError::invalid_arguments("this session is not on a team")
            })?;
        let member = require_member(&team, &sid)?;
        let claimed = claim_task(&store, &team.team_id, &input.task_id, &member.name)
            .map_err(team_error)?;
        Ok(TeamTaskClaimOutput {
            id: claimed.id,
            assignee: claimed.assignee.unwrap_or_default(),
        })
    }
}

// --- team_task_complete ---

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TeamTaskCompleteInput {
    pub task_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TeamTaskCompleteOutput {
    pub id: String,
}

impl xai_tool_runtime::ToolOutput for TeamTaskCompleteOutput {}

#[derive(Debug, Default)]
pub struct TeamTaskCompleteTool;

impl crate::types::tool_metadata::ToolMetadata for TeamTaskCompleteTool {
    fn kind(&self) -> ToolKind {
        ToolKind::Other
    }
    fn tool_namespace(&self) -> ToolNamespace {
        ToolNamespace::GrokBuild
    }
    fn description_template(&self) -> &str {
        "Mark a team task completed. Unblocks dependents so another member can claim them."
    }
    fn requires_expr(&self) -> Expr<ToolRequirement> {
        Expr::True
    }
}

impl xai_tool_runtime::Tool for TeamTaskCompleteTool {
    type Args = TeamTaskCompleteInput;
    type Output = TeamTaskCompleteOutput;

    fn id(&self) -> xai_tool_protocol::ToolId {
        xai_tool_protocol::ToolId::new(TEAM_TASK_COMPLETE_TOOL_NAME).expect("valid tool id")
    }

    fn description(
        &self,
        _ctx: &xai_tool_runtime::ListToolsContext,
    ) -> xai_tool_types::ToolDescription {
        xai_tool_types::ToolDescription::new(
            TEAM_TASK_COMPLETE_TOOL_NAME,
            crate::types::tool_metadata::ToolMetadata::sanitized_description_template(self),
        )
    }

    fn capabilities(&self) -> xai_tool_protocol::ToolCapabilities {
        xai_tool_protocol::ToolCapabilities {
            is_read_only: false,
            tool_scope: Some(xai_tool_protocol::ToolScope::Write),
            ..Default::default()
        }
    }

    #[tracing::instrument(name = "tool.team_task_complete", skip_all)]
    async fn run(
        &self,
        ctx: xai_tool_runtime::ToolCallContext,
        input: TeamTaskCompleteInput,
    ) -> Result<TeamTaskCompleteOutput, xai_tool_runtime::ToolError> {
        let sid = session_id(&ctx)?;
        let store = store();
        let team = store
            .find_by_session(&sid)
            .map_err(team_error)?
            .ok_or_else(|| {
                xai_tool_runtime::ToolError::invalid_arguments("this session is not on a team")
            })?;
        require_member(&team, &sid)?;
        let done = complete_task(&store, &team.team_id, &input.task_id).map_err(team_error)?;
        Ok(TeamTaskCompleteOutput { id: done.id })
    }
}

// --- team_status ---

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TeamStatusInput {}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TeamStatusOutput {
    pub team_id: String,
    pub members: Vec<String>,
    pub pending_spawns: Vec<String>,
    pub tasks: usize,
    pub you: String,
}

impl xai_tool_runtime::ToolOutput for TeamStatusOutput {}

#[derive(Debug, Default)]
pub struct TeamStatusTool;

impl crate::types::tool_metadata::ToolMetadata for TeamStatusTool {
    fn kind(&self) -> ToolKind {
        ToolKind::Other
    }
    fn tool_namespace(&self) -> ToolNamespace {
        ToolNamespace::GrokBuild
    }
    fn description_template(&self) -> &str {
        "Snapshot of this team's members, pending spawns, and task count."
    }
    fn requires_expr(&self) -> Expr<ToolRequirement> {
        Expr::True
    }
}

impl xai_tool_runtime::Tool for TeamStatusTool {
    type Args = TeamStatusInput;
    type Output = TeamStatusOutput;

    fn id(&self) -> xai_tool_protocol::ToolId {
        xai_tool_protocol::ToolId::new(TEAM_STATUS_TOOL_NAME).expect("valid tool id")
    }

    fn description(
        &self,
        _ctx: &xai_tool_runtime::ListToolsContext,
    ) -> xai_tool_types::ToolDescription {
        xai_tool_types::ToolDescription::new(
            TEAM_STATUS_TOOL_NAME,
            crate::types::tool_metadata::ToolMetadata::sanitized_description_template(self),
        )
    }

    fn capabilities(&self) -> xai_tool_protocol::ToolCapabilities {
        xai_tool_protocol::ToolCapabilities {
            is_read_only: true,
            tool_scope: Some(xai_tool_protocol::ToolScope::Read),
            ..Default::default()
        }
    }

    #[tracing::instrument(name = "tool.team_status", skip_all)]
    async fn run(
        &self,
        ctx: xai_tool_runtime::ToolCallContext,
        _input: TeamStatusInput,
    ) -> Result<TeamStatusOutput, xai_tool_runtime::ToolError> {
        let sid = session_id(&ctx)?;
        let store = store();
        let team = store
            .find_by_session(&sid)
            .map_err(team_error)?
            .ok_or_else(|| {
                xai_tool_runtime::ToolError::invalid_arguments("this session is not on a team")
            })?;
        let you = require_member(&team, &sid)?.name.clone();
        let pending_spawns: Vec<String> = team
            .members
            .iter()
            .filter(|m| m.role == MemberRole::Teammate && m.session_id.is_none())
            .map(|m| m.name.clone())
            .collect();
        let tasks = store.read_tasks(&team.team_id).map_err(team_error)?.len();
        Ok(TeamStatusOutput {
            team_id: team.team_id,
            members: team.members.iter().map(|m| m.name.clone()).collect(),
            pending_spawns,
            tasks,
            you,
        })
    }
}

/// True when `id` is a team tool (`GrokBuild:spawn_teammate` or bare name).
pub fn is_team_tool_id(id: &str) -> bool {
    TEAM_TOOL_NAMES.iter().any(|name| {
        id == *name || id.ends_with(&format!(":{name}"))
    })
}

#[cfg(test)]
mod tool_tests {
    use super::*;
    use xai_tool_runtime::Tool as _;

    fn ctx(session: &str) -> xai_tool_runtime::ToolCallContext {
        let mut ctx = xai_tool_runtime::ToolCallContext::default();
        ctx.insert(xai_tool_runtime::SessionContext(session.to_string()));
        ctx
    }

    #[test]
    fn team_id_helper_stable() {
        assert_eq!(
            crate::implementations::grok_build::team::team_id_from_session(
                "019ffb5b-ad0b-7833-8305-0d9b097d7e74"
            ),
            "session-019ffb5b"
        );
    }

    #[test]
    fn is_team_tool_id_matches_qualified_and_bare() {
        assert!(is_team_tool_id("spawn_teammate"));
        assert!(is_team_tool_id("GrokBuild:send_message"));
        assert!(!is_team_tool_id("spawn_subagent"));
        assert!(!is_team_tool_id("task"));
    }

    #[tokio::test]
    async fn spawn_rejects_bad_name() {
        let err = SpawnTeammateTool
            .run(
                ctx("sess-1"),
                SpawnTeammateInput {
                    name: "Reviewer".into(),
                    prompt: "review".into(),
                },
            )
            .await
            .unwrap_err();
        assert!(err.to_string().contains("[a-z]"));
    }
}