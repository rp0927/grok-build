use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use xai_grok_config::fs_atomic::write_atomically;

use super::types::{MemberRole, TeamConfig, TeamMember, TeamTask, team_id_from_session};

#[derive(Debug, thiserror::Error)]
pub enum TeamStoreError {
    #[error("io: {0}")]
    Io(#[from] io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("team already exists: {0}")]
    AlreadyExists(String),
    #[error("team not found: {0}")]
    NotFound(String),
    #[error("unknown member: {0}")]
    UnknownMember(String),
    #[error("duplicate member name: {0}")]
    DuplicateMember(String),
}

/// File-backed team directory under `{root}/teams/{team_id}`.
#[derive(Debug, Clone)]
pub struct TeamStore {
    root: PathBuf,
}

impl TeamStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn team_dir(&self, team_id: &str) -> PathBuf {
        self.root.join("teams").join(team_id)
    }

    pub fn inbox_path(&self, team_id: &str, member: &str) -> PathBuf {
        self.team_dir(team_id)
            .join("inboxes")
            .join(format!("{member}.json"))
    }

    pub fn tasks_path(&self, team_id: &str) -> PathBuf {
        self.team_dir(team_id).join("tasks.json")
    }

    pub fn config_path(&self, team_id: &str) -> PathBuf {
        self.team_dir(team_id).join("config.json")
    }

    pub fn create_for_lead(
        &self,
        lead_session_id: &str,
        lead_name: &str,
        now_unix_ms: i64,
    ) -> Result<TeamConfig, TeamStoreError> {
        let team_id = team_id_from_session(lead_session_id);
        let dir = self.team_dir(&team_id);
        if dir.exists() {
            return Err(TeamStoreError::AlreadyExists(team_id));
        }
        fs::create_dir_all(dir.join("inboxes"))?;
        let config = TeamConfig {
            team_id: team_id.clone(),
            lead_session_id: lead_session_id.to_string(),
            members: vec![TeamMember {
                name: lead_name.to_string(),
                role: MemberRole::TeamLead,
                session_id: Some(lead_session_id.to_string()),
                spawn_prompt: None,
            }],
            created_unix_ms: now_unix_ms,
        };
        self.write_config(&config)?;
        self.write_tasks(&team_id, &[])?;
        self.write_inbox(&team_id, lead_name, &[])?;
        Ok(config)
    }

    pub fn load(&self, team_id: &str) -> Result<TeamConfig, TeamStoreError> {
        let path = self.config_path(team_id);
        if !path.exists() {
            return Err(TeamStoreError::NotFound(team_id.to_string()));
        }
        let raw = fs::read_to_string(path)?;
        Ok(serde_json::from_str(&raw)?)
    }

    pub fn add_member(
        &self,
        team_id: &str,
        name: &str,
        session_id: Option<String>,
        spawn_prompt: Option<String>,
    ) -> Result<TeamConfig, TeamStoreError> {
        let mut config = self.load(team_id)?;
        if config.member(name).is_some() {
            return Err(TeamStoreError::DuplicateMember(name.to_string()));
        }
        config.members.push(TeamMember {
            name: name.to_string(),
            role: MemberRole::Teammate,
            session_id,
            spawn_prompt,
        });
        self.write_config(&config)?;
        self.write_inbox(team_id, name, &[])?;
        Ok(config)
    }

    /// Scan `$root/teams/*/config.json` for a member bound to `session_id`.
    pub fn find_by_session(&self, session_id: &str) -> Result<Option<TeamConfig>, TeamStoreError> {
        let teams = self.root.join("teams");
        if !teams.is_dir() {
            return Ok(None);
        }
        for entry in fs::read_dir(teams)? {
            let path = entry?.path().join("config.json");
            if !path.exists() {
                continue;
            }
            let cfg: TeamConfig = serde_json::from_str(&fs::read_to_string(path)?)?;
            if cfg.member_for_session(session_id).is_some() {
                return Ok(Some(cfg));
            }
        }
        Ok(None)
    }

    pub fn bind_session(
        &self,
        team_id: &str,
        name: &str,
        session_id: &str,
    ) -> Result<TeamConfig, TeamStoreError> {
        let mut config = self.load(team_id)?;
        let member = config
            .members
            .iter_mut()
            .find(|m| m.name == name)
            .ok_or_else(|| TeamStoreError::UnknownMember(name.to_string()))?;
        member.session_id = Some(session_id.to_string());
        member.spawn_prompt = None;
        self.write_config(&config)?;
        Ok(config)
    }

    pub fn cursor_path(&self, team_id: &str, member: &str) -> PathBuf {
        self.team_dir(team_id)
            .join("inboxes")
            .join(format!("{member}.cursor"))
    }

    pub fn read_cursor(
        &self,
        team_id: &str,
        member: &str,
    ) -> Result<Option<String>, TeamStoreError> {
        let path = self.cursor_path(team_id, member);
        if !path.exists() {
            return Ok(None);
        }
        let raw = fs::read_to_string(path)?;
        let id = raw.trim();
        if id.is_empty() {
            Ok(None)
        } else {
            Ok(Some(id.to_string()))
        }
    }

    pub fn write_cursor(
        &self,
        team_id: &str,
        member: &str,
        last_id: &str,
    ) -> Result<(), TeamStoreError> {
        let path = self.cursor_path(team_id, member);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        write_atomically(&path, last_id, Some(0o600))?;
        Ok(())
    }

    pub fn write_config(&self, config: &TeamConfig) -> Result<(), TeamStoreError> {
        let path = self.config_path(&config.team_id);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let body = serde_json::to_string_pretty(config)?;
        write_atomically(&path, &body, Some(0o600))?;
        Ok(())
    }

    pub fn write_inbox(
        &self,
        team_id: &str,
        member: &str,
        messages: &[super::types::MailboxMessage],
    ) -> Result<(), TeamStoreError> {
        let path = self.inbox_path(team_id, member);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let body = serde_json::to_string_pretty(messages)?;
        write_atomically(&path, &body, Some(0o600))?;
        Ok(())
    }

    pub fn read_inbox(
        &self,
        team_id: &str,
        member: &str,
    ) -> Result<Vec<super::types::MailboxMessage>, TeamStoreError> {
        let config = self.load(team_id)?;
        if config.member(member).is_none() {
            return Err(TeamStoreError::UnknownMember(member.to_string()));
        }
        let path = self.inbox_path(team_id, member);
        if !path.exists() {
            return Ok(Vec::new());
        }
        let raw = fs::read_to_string(path)?;
        Ok(serde_json::from_str(&raw)?)
    }

    pub fn write_tasks(&self, team_id: &str, tasks: &[TeamTask]) -> Result<(), TeamStoreError> {
        let path = self.tasks_path(team_id);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let body = serde_json::to_string_pretty(tasks)?;
        write_atomically(&path, &body, Some(0o600))?;
        Ok(())
    }

    pub fn read_tasks(&self, team_id: &str) -> Result<Vec<TeamTask>, TeamStoreError> {
        let path = self.tasks_path(team_id);
        if !path.exists() {
            return Ok(Vec::new());
        }
        let raw = fs::read_to_string(path)?;
        Ok(serde_json::from_str(&raw)?)
    }

    pub fn exists(path: &Path) -> bool {
        path.exists()
    }
}
