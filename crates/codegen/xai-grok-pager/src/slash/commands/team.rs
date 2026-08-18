//! `/team` — snapshot the experimental agent team (members, tasks, unread).
//!
//! Hidden unless `GROK_EXPERIMENTAL_AGENT_TEAMS=1` or `[features] agent_teams`.
//! Teammates are top-level sessions, not `spawn_subagent` children.

use crate::app::team_runtime;
use crate::slash::command::{CommandExecCtx, CommandResult, SlashCommand};

/// Show the current session's agent-team roster.
pub struct TeamCommand;

impl SlashCommand for TeamCommand {
    fn name(&self) -> &str {
        "team"
    }

    fn description(&self) -> &str {
        "Show this session's agent team (experimental)"
    }

    fn session_scoped(&self) -> bool {
        true
    }

    fn usage(&self) -> &str {
        "/team [status]"
    }

    fn takes_args(&self) -> bool {
        true
    }

    fn args_required(&self) -> bool {
        false
    }

    fn visible(&self, _ctx: &crate::slash::command::AppCtx) -> bool {
        team_runtime::enabled()
    }

    fn run(&self, ctx: &mut CommandExecCtx, args: &str) -> CommandResult {
        if !team_runtime::enabled() {
            return CommandResult::Error(
                "Agent teams are off. Set GROK_EXPERIMENTAL_AGENT_TEAMS=1 or [features] agent_teams = true."
                    .to_string(),
            );
        }
        let Some(sid) = ctx.session_id else {
            return CommandResult::Error("No active session".to_string());
        };
        let arg = args.trim();
        if !arg.is_empty() && arg != "status" && arg != "help" {
            return CommandResult::Message(
                "Usage: /team [status]\nShows members, unread mailbox counts, and shared tasks."
                    .to_string(),
            );
        }
        if arg == "help" {
            return CommandResult::Message(
                "Usage: /team [status]\n\
Teammates are top-level sessions (not subagents). \
The lead calls spawn_teammate; peers use send_message and team_task_*."
                    .to_string(),
            );
        }
        CommandResult::Message(team_runtime::format_team_status(
            &team_runtime::store(),
            sid.0.as_ref(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::acp::model_state::ModelState;
    use crate::app::bundle::BundleState;
    use crate::settings::PagerLocalSnapshot;

    static DEFAULT_BUNDLE_STATE: BundleState = BundleState {
        has_cache: false,
        version: String::new(),
        personas: Vec::new(),
        roles: Vec::new(),
        agents: Vec::new(),
        skills: Vec::new(),
        persona_details: Vec::new(),
        role_details: Vec::new(),
    };

    fn run(sid: Option<&agent_client_protocol::SessionId>, args: &str) -> CommandResult {
        let models = ModelState::default();
        let mut ctx = CommandExecCtx {
            models: &models,
            session_id: sid,
            bundle_state: &DEFAULT_BUNDLE_STATE,
            screen_mode: crate::app::ScreenMode::Minimal,
            billing_surface_visible: true,
            usage_command_visible: true,
            pager_state: PagerLocalSnapshot::default(),
        };
        TeamCommand.run(&mut ctx, args)
    }

    #[test]
    fn no_session_errors() {
        match run(None, "") {
            CommandResult::Error(msg) => assert!(msg.contains("No active session") || msg.contains("off")),
            other => panic!("expected Error, got {other:?}"),
        }
    }

    #[test]
    fn help_arg_prints_usage() {
        if !team_runtime::enabled() {
            return;
        }
        let sid = agent_client_protocol::SessionId::from("s1".to_string());
        match run(Some(&sid), "help") {
            CommandResult::Message(msg) => assert!(msg.contains("spawn_teammate"), "{msg}"),
            other => panic!("expected Message, got {other:?}"),
        }
    }

    #[test]
    fn unknown_arg_prints_usage() {
        if !team_runtime::enabled() {
            return;
        }
        let sid = agent_client_protocol::SessionId::from("s1".to_string());
        match run(Some(&sid), "wat") {
            CommandResult::Message(msg) => assert!(msg.contains("Usage"), "{msg}"),
            other => panic!("expected Message, got {other:?}"),
        }
    }
}
