//! Pager-side agent-team orchestration.
//!
//! Tools persist pending members (`session_id: None` + `spawn_prompt`). This
//! module turns those rows into top-level pager sessions, binds the ACP
//! session id, and injects mailbox traffic as tagged user turns.

use std::collections::HashSet;
use std::time::Instant;

use xai_grok_shell::agent::team::{MailboxMessage, MemberRole, TeamConfig, TeamStore};

use crate::app::agent::AgentId;
use crate::app::agent_view::AgentView;
use crate::app::app_view::AppView;
use crate::app::dispatch::dispatch_new_session_inner_with_id;
use crate::views::team_panel::{
    TeamActivity, TeamPanelRow, TeamSnapshot, TeamTaskSummary,
};

pub fn enabled() -> bool {
    match xai_grok_shell::config::load_effective_config() {
        Ok(cfg) => xai_grok_config::agent_teams_enabled(&cfg),
        Err(_) => matches!(
            std::env::var("GROK_EXPERIMENTAL_AGENT_TEAMS").as_deref(),
            Ok("1") | Ok("true") | Ok("TRUE") | Ok("yes")
        ),
    }
}

pub fn store() -> TeamStore {
    TeamStore::new(xai_grok_config::grok_home())
}

pub fn pending_to_materialize(
    team: &TeamConfig,
    in_flight: &HashSet<String>,
) -> Vec<(String, String)> {
    team.members
        .iter()
        .filter(|m| {
            m.role == MemberRole::Teammate
                && m.session_id.is_none()
                && m.spawn_prompt.is_some()
                && !in_flight.contains(&m.name)
        })
        .filter_map(|m| m.spawn_prompt.clone().map(|p| (m.name.clone(), p)))
        .collect()
}

pub fn mailbox_delivery_prompt(msg: &MailboxMessage) -> String {
    let header = format!("[from teammate {}, not the human]", msg.from);
    match msg.subject.as_deref() {
        Some(subject) if !subject.trim().is_empty() => {
            format!("{header}\n{subject}\n\n{}", msg.body)
        }
        _ => format!("{header}\n{}", msg.body),
    }
}

pub fn undelivered<'a>(
    inbox: &'a [MailboxMessage],
    cursor: Option<&str>,
) -> &'a [MailboxMessage] {
    let Some(cursor) = cursor else {
        return inbox;
    };
    match inbox.iter().position(|m| m.id == cursor) {
        Some(i) => &inbox[i + 1..],
        None => inbox,
    }
}

pub fn activity_for_agent(agent: &AgentView) -> TeamActivity {
    if !agent.permission_queue.is_empty() || agent.question_view.is_some() {
        return TeamActivity::Blocked;
    }
    if agent.session.state.is_busy() {
        return TeamActivity::Working;
    }
    TeamActivity::Idle
}

pub fn snapshot_for_session(
    store: &TeamStore,
    session_id: &str,
    live: &std::collections::HashMap<String, TeamActivity>,
    in_flight: &HashSet<String>,
) -> Option<TeamSnapshot> {
    let team = store.find_by_session(session_id).ok().flatten()?;
    let lead = team.lead()?;
    if lead.session_id.as_deref() != Some(session_id) && team.lead_session_id != session_id {
        return None;
    }
    let mut unread_total = 0u32;
    let mut rows = Vec::with_capacity(team.members.len());
    for member in &team.members {
        let inbox = store.read_inbox(&team.team_id, &member.name).unwrap_or_default();
        let cursor = store.read_cursor(&team.team_id, &member.name).ok().flatten();
        let unread = undelivered(&inbox, cursor.as_deref()).len() as u32;
        unread_total = unread_total.saturating_add(unread);
        let spawn_pending = member.session_id.is_none()
            && (member.spawn_prompt.is_some() || in_flight.contains(&member.name));
        let activity = if spawn_pending {
            TeamActivity::Pending
        } else if let Some(sid) = member.session_id.as_deref() {
            live.get(sid).copied().unwrap_or(TeamActivity::Idle)
        } else {
            TeamActivity::Pending
        };
        rows.push(TeamPanelRow {
            name: member.name.clone(),
            is_lead: member.role == MemberRole::TeamLead,
            activity,
            unread,
            session_id: member.session_id.clone(),
            spawn_pending,
        });
    }
    let tasks = store
        .read_tasks(&team.team_id)
        .unwrap_or_default()
        .into_iter()
        .map(|t| TeamTaskSummary {
            id: t.id,
            title: t.title,
            status: format!("{:?}", t.status).to_ascii_lowercase(),
        })
        .collect();
    Some(TeamSnapshot {
        team_id: team.team_id,
        rows,
        tasks,
        unread_total,
    })
}

pub fn refresh_all_panels(app: &mut AppView) {
    if !enabled() {
        for agent in app.agents.values_mut() {
            agent.team.apply_snapshot(None, Instant::now());
        }
        return;
    }
    let live: std::collections::HashMap<String, TeamActivity> = app
        .agents
        .values()
        .filter_map(|a| {
            let sid = a.session.session_id.as_ref()?.0.to_string();
            Some((sid, activity_for_agent(a)))
        })
        .collect();
    let store = store();
    for agent in app.agents.values_mut() {
        if agent.is_subagent_view {
            agent.team.apply_snapshot(None, Instant::now());
            continue;
        }
        let Some(sid) = agent.session.session_id.as_ref().map(|s| s.0.to_string()) else {
            agent.team.apply_snapshot(None, Instant::now());
            continue;
        };
        let snap = snapshot_for_session(&store, &sid, &live, &agent.team.in_flight);
        agent.team.apply_snapshot(snap, Instant::now());
    }
}

/// Create top-level sessions for pending `spawn_teammate` rows and deliver
/// unread mailbox messages to idle members. Effects are appended to
/// `app.pending_effects`.
pub fn sync(app: &mut AppView) {
    if !enabled() {
        return;
    }
    let store = store();
    materialize_pending(app, &store);
    deliver_mailboxes(app, &store);
    refresh_all_panels(app);
}

pub fn materialize_pending(app: &mut AppView, store: &TeamStore) {
    let leads: Vec<(AgentId, String, HashSet<String>)> = app
        .agents
        .iter()
        .filter(|(_, a)| !a.is_subagent_view)
        .filter_map(|(id, a)| {
            let sid = a.session.session_id.as_ref()?.0.to_string();
            Some((*id, sid, a.team.in_flight.clone()))
        })
        .collect();
    for (lead_id, sid, in_flight) in leads {
        let Ok(Some(team)) = store.find_by_session(&sid) else {
            continue;
        };
        let is_lead = team.lead_session_id == sid
            || team
                .lead()
                .and_then(|m| m.session_id.as_deref())
                == Some(sid.as_str());
        if !is_lead {
            continue;
        }
        for (name, prompt) in pending_to_materialize(&team, &in_flight) {
            spawn_teammate_session(app, store, &team.team_id, &name, &prompt);
            if let Some(lead) = app.agents.get_mut(&lead_id) {
                lead.team.in_flight.insert(name);
            }
        }
    }
}

fn spawn_teammate_session(
    app: &mut AppView,
    store: &TeamStore,
    team_id: &str,
    name: &str,
    prompt: &str,
) {
    let previous = app.active_view;
    let (new_id, effects) = dispatch_new_session_inner_with_id(app, None);
    // Stay on the lead. dispatch_new_session_inner_with_id switches the
    // active view; put it back so spawning a teammate is not a focus steal.
    app.active_view = previous;
    if let Some(agent) = app.agents.get_mut(&new_id) {
        agent.team_bind = Some((team_id.to_string(), name.to_string()));
        agent.session.enqueue_prompt(team_spawn_prompt(name, prompt));
    }
    let _ = store;
    app.pending_effects.extend(effects);
}

fn team_spawn_prompt(name: &str, prompt: &str) -> String {
    format!(
        "You are teammate @{name} on an agent team. You are a full top-level session, not a subagent. \
Use send_message to talk to other members by name. Do not wait for the human to relay.\n\n{prompt}"
    )
}

pub fn on_session_bound(app: &mut AppView, agent_id: AgentId) {
    if !enabled() {
        return;
    }
    let Some(agent) = app.agents.get(&agent_id) else {
        return;
    };
    let Some((team_id, name)) = agent.team_bind.clone() else {
        return;
    };
    let Some(sid) = agent.session.session_id.as_ref().map(|s| s.0.to_string()) else {
        return;
    };
    let store = store();
    if store.bind_session(&team_id, &name, &sid).is_ok()
        && let Some(lead) = app.agents.values_mut().find(|a| {
            a.team.team_id.as_deref() == Some(team_id.as_str()) || a.team.in_flight.contains(&name)
        })
    {
        lead.team.in_flight.remove(&name);
    }
}

pub fn deliver_mailboxes(app: &mut AppView, store: &TeamStore) {
    let members: Vec<(AgentId, String, String, String, bool)> = app
        .agents
        .iter()
        .filter_map(|(id, a)| {
            let sid = a.session.session_id.as_ref()?.0.to_string();
            let team = store.find_by_session(&sid).ok().flatten()?;
            let member = team.member_for_session(&sid)?;
            Some((
                *id,
                team.team_id.clone(),
                member.name.clone(),
                sid,
                a.session.state.is_idle(),
            ))
        })
        .collect();
    for (agent_id, team_id, name, _sid, idle) in members {
        let Ok(inbox) = store.read_inbox(&team_id, &name) else {
            continue;
        };
        let cursor = store.read_cursor(&team_id, &name).ok().flatten();
        let fresh = undelivered(&inbox, cursor.as_deref());
        if fresh.is_empty() {
            continue;
        }
        let last_id = fresh.last().map(|m| m.id.clone());
        let prompts: Vec<String> = fresh.iter().map(mailbox_delivery_prompt).collect();
        if let Some(agent) = app.agents.get_mut(&agent_id) {
            // Always enqueue; the existing queue drain sends when idle (auto-wake).
            for prompt in prompts {
                agent.session.enqueue_prompt(prompt);
            }
            let _ = idle;
        }
        if let Some(id) = last_id {
            let _ = store.write_cursor(&team_id, &name, &id);
        }
    }
}

pub fn agent_id_for_session(app: &AppView, session_id: &str) -> Option<AgentId> {
    app.agents.iter().find_map(|(id, a)| {
        a.session
            .session_id
            .as_ref()
            .is_some_and(|s| s.0.as_ref() == session_id)
            .then_some(*id)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use xai_grok_shell::agent::team::{MailboxKind, send_message};

    #[test]
    fn pending_skips_in_flight_and_bound() {
        let dir = tempfile::tempdir().unwrap();
        let store = TeamStore::new(dir.path());
        let team = store.create_for_lead("lead-sid", "lead", 1).unwrap();
        store
            .add_member(&team.team_id, "reviewer", None, Some("review the diff".into()))
            .unwrap();
        store
            .add_member(&team.team_id, "writer", Some("already".into()), None)
            .unwrap();
        let team = store.load(&team.team_id).unwrap();
        let pending = pending_to_materialize(&team, &HashSet::new());
        assert_eq!(pending, vec![("reviewer".into(), "review the diff".into())]);
        let mut inflight = HashSet::new();
        inflight.insert("reviewer".into());
        assert!(pending_to_materialize(&team, &inflight).is_empty());
    }

    #[test]
    fn mailbox_prompt_tags_sender() {
        let msg = MailboxMessage {
            id: "m1".into(),
            from: "reviewer".into(),
            to: "lead".into(),
            kind: MailboxKind::Text,
            subject: Some("nits".into()),
            body: "please rename foo".into(),
            created_unix_ms: 1,
        };
        let text = mailbox_delivery_prompt(&msg);
        assert!(text.contains("[from teammate reviewer, not the human]"));
        assert!(text.contains("nits"));
        assert!(text.contains("please rename foo"));
    }

    #[test]
    fn undelivered_after_cursor() {
        let msgs = vec![
            MailboxMessage {
                id: "a".into(),
                from: "lead".into(),
                to: "r".into(),
                kind: MailboxKind::Text,
                subject: None,
                body: "1".into(),
                created_unix_ms: 1,
            },
            MailboxMessage {
                id: "b".into(),
                from: "lead".into(),
                to: "r".into(),
                kind: MailboxKind::Text,
                subject: None,
                body: "2".into(),
                created_unix_ms: 2,
            },
        ];
        assert_eq!(undelivered(&msgs, None).len(), 2);
        assert_eq!(undelivered(&msgs, Some("a")).len(), 1);
        assert_eq!(undelivered(&msgs, Some("b")).len(), 0);
    }

    #[test]
    fn snapshot_hides_from_non_lead() {
        let dir = tempfile::tempdir().unwrap();
        let store = TeamStore::new(dir.path());
        let team = store.create_for_lead("lead-sid", "lead", 1).unwrap();
        store
            .add_member(&team.team_id, "reviewer", Some("rev-sid".into()), None)
            .unwrap();
        let live = std::collections::HashMap::new();
        let inflight = HashSet::new();
        assert!(snapshot_for_session(&store, "rev-sid", &live, &inflight).is_none());
        let snap = snapshot_for_session(&store, "lead-sid", &live, &inflight).unwrap();
        assert_eq!(snap.rows.len(), 2);
        assert!(snap.rows.iter().any(|r| r.name == "reviewer"));
    }

    #[test]
    fn send_then_undelivered_tracks_cursor() {
        let dir = tempfile::tempdir().unwrap();
        let store = TeamStore::new(dir.path());
        let team = store.create_for_lead("s1", "lead", 1).unwrap();
        store.add_member(&team.team_id, "reviewer", None, None).unwrap();
        send_message(
            &store,
            &team.team_id,
            "lead",
            "reviewer",
            MailboxKind::Text,
            None,
            "hi".into(),
            2,
            "m1".into(),
        )
        .unwrap();
        let inbox = store.read_inbox(&team.team_id, "reviewer").unwrap();
        assert_eq!(undelivered(&inbox, None).len(), 1);
        store.write_cursor(&team.team_id, "reviewer", "m1").unwrap();
        let cursor = store.read_cursor(&team.team_id, "reviewer").unwrap();
        let inbox = store.read_inbox(&team.team_id, "reviewer").unwrap();
        assert!(undelivered(&inbox, cursor.as_deref()).is_empty());
    }
}
