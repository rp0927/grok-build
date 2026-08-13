//! In-process agent-team panel (Claude-style row list above the prompt).
//!
//! Pure layout + paint. Membership and mailbox live in
//! `xai_grok_shell::agent::team`. The pager attaches this band only when the
//! experimental flag is on and the current session is a team lead.

use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::Span;

use crate::theme::Theme;

/// Hide idle teammate rows this long after the whole team goes idle.
pub const IDLE_COLLAPSE_AFTER: Duration = Duration::from_secs(30);

/// Header (1) + at most this many member rows + footer (1).
const MAX_MEMBER_ROWS: usize = 6;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TeamActivity {
    Pending,
    Working,
    Blocked,
    Idle,
    Done,
}

impl TeamActivity {
    pub fn label(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Working => "working",
            Self::Blocked => "blocked",
            Self::Idle => "idle",
            Self::Done => "done",
        }
    }

    pub fn is_busy(self) -> bool {
        matches!(self, Self::Pending | Self::Working | Self::Blocked)
    }

    pub fn is_idle_like(self) -> bool {
        matches!(self, Self::Idle | Self::Done)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TeamPanelRow {
    pub name: String,
    pub is_lead: bool,
    pub activity: TeamActivity,
    pub unread: u32,
    pub session_id: Option<String>,
    pub spawn_pending: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TeamTaskSummary {
    pub id: String,
    pub title: String,
    pub status: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TeamSnapshot {
    pub team_id: String,
    pub rows: Vec<TeamPanelRow>,
    pub tasks: Vec<TeamTaskSummary>,
    pub unread_total: u32,
}

/// Outcome of a key while the team pane is focused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TeamPanelKey {
    SelectChanged,
    Open { session_id: String },
    Interrupt { session_id: String },
    ToggleTasks,
    FocusPrompt,
    Ignored,
}

#[derive(Debug, Clone)]
pub struct TeamPanel {
    pub selected: usize,
    pub last_all_idle_at: Option<Instant>,
    pub rows: Vec<TeamPanelRow>,
    pub tasks: Vec<TeamTaskSummary>,
    pub show_tasks: bool,
    pub unread_total: u32,
    pub team_id: Option<String>,
    /// Member names this pager has already started materializing.
    pub in_flight: std::collections::HashSet<String>,
}

impl Default for TeamPanel {
    fn default() -> Self {
        Self::new()
    }
}

impl TeamPanel {
    pub fn new() -> Self {
        Self {
            selected: 0,
            last_all_idle_at: None,
            rows: Vec::new(),
            tasks: Vec::new(),
            show_tasks: false,
            unread_total: 0,
            team_id: None,
            in_flight: std::collections::HashSet::new(),
        }
    }

    pub fn is_visible(&self) -> bool {
        self.desired_height(u16::MAX) > 0
    }

    pub fn apply_snapshot(&mut self, snap: Option<TeamSnapshot>, now: Instant) {
        match snap {
            None => {
                self.rows.clear();
                self.tasks.clear();
                self.unread_total = 0;
                self.team_id = None;
                self.last_all_idle_at = None;
                self.selected = 0;
            }
            Some(snap) => {
                let all_idle = !snap.rows.is_empty()
                    && snap.rows.iter().all(|r| r.activity.is_idle_like());
                if all_idle {
                    if self.last_all_idle_at.is_none() {
                        self.last_all_idle_at = Some(now);
                    }
                } else {
                    self.last_all_idle_at = None;
                }
                self.team_id = Some(snap.team_id);
                self.rows = snap.rows;
                self.tasks = snap.tasks;
                self.unread_total = snap.unread_total;
                let visible = visible_row_indices(&self.rows, self.last_all_idle_at, now);
                if visible.is_empty() {
                    self.selected = 0;
                } else if !visible.contains(&self.selected) {
                    self.selected = visible[0];
                }
            }
        }
    }

    pub fn desired_height(&self, max_area: u16) -> u16 {
        if self.rows.iter().all(|r| r.is_lead) || self.rows.is_empty() {
            return 0;
        }
        let now = Instant::now();
        let visible = visible_row_indices(&self.rows, self.last_all_idle_at, now);
        if visible.is_empty() && self.last_all_idle_at.is_some() {
            // Header-only collapsed strip so the lead still sees membership.
            return 1.min(max_area);
        }
        let mut rows = 1usize + visible.len().min(MAX_MEMBER_ROWS);
        if self.show_tasks && !self.tasks.is_empty() {
            rows += self.tasks.len().min(4);
        }
        rows += 1; // footer
        (rows as u16).min(max_area).min(12)
    }

    pub fn selected_row(&self) -> Option<&TeamPanelRow> {
        self.rows.get(self.selected)
    }

    pub fn handle_key(&mut self, key: &KeyEvent) -> TeamPanelKey {
        if key.modifiers.is_empty() {
            match key.code {
                KeyCode::Up => {
                    self.select_delta(-1);
                    return TeamPanelKey::SelectChanged;
                }
                KeyCode::Down => {
                    self.select_delta(1);
                    return TeamPanelKey::SelectChanged;
                }
                KeyCode::Enter => {
                    if let Some(row) = self.selected_row()
                        && let Some(sid) = row.session_id.clone()
                        && !row.is_lead
                    {
                        return TeamPanelKey::Open { session_id: sid };
                    }
                    return TeamPanelKey::Ignored;
                }
                KeyCode::Char('x') => {
                    if let Some(row) = self.selected_row()
                        && row.activity == TeamActivity::Working
                        && let Some(sid) = row.session_id.clone()
                    {
                        return TeamPanelKey::Interrupt { session_id: sid };
                    }
                    return TeamPanelKey::Ignored;
                }
                KeyCode::Esc | KeyCode::Tab => return TeamPanelKey::FocusPrompt,
                _ => {}
            }
        }
        if crate::key!('t', CONTROL).matches(key) {
            self.show_tasks = !self.show_tasks;
            return TeamPanelKey::ToggleTasks;
        }
        TeamPanelKey::Ignored
    }

    fn select_delta(&mut self, delta: i32) {
        let now = Instant::now();
        let visible = visible_row_indices(&self.rows, self.last_all_idle_at, now);
        if visible.is_empty() {
            return;
        }
        let pos = visible.iter().position(|&i| i == self.selected).unwrap_or(0);
        let next = if delta < 0 {
            pos.saturating_sub(1)
        } else {
            (pos + 1).min(visible.len() - 1)
        };
        self.selected = visible[next];
    }

    pub fn render(&self, area: Rect, buf: &mut Buffer, focused: bool) {
        if area.width == 0 || area.height == 0 {
            return;
        }
        let theme = Theme::current();
        let now = Instant::now();
        let visible = visible_row_indices(&self.rows, self.last_all_idle_at, now);
        let header = format!(
            " team {} · {}  ",
            self.rows.len(),
            if self.unread_total > 0 {
                format!("{} unread", self.unread_total)
            } else if visible.is_empty() {
                "all idle".to_string()
            } else {
                "↑↓ enter x".to_string()
            }
        );
        paint_line(
            buf,
            area.x,
            area.y,
            area.width,
            &header,
            Style::default()
                .fg(theme.text_secondary)
                .add_modifier(if focused {
                    Modifier::BOLD
                } else {
                    Modifier::empty()
                }),
            theme.bg_base,
        );
        if area.height == 1 {
            return;
        }
        let mut y = area.y.saturating_add(1);
        let bottom = area.y.saturating_add(area.height);
        for &idx in visible.iter().take(MAX_MEMBER_ROWS) {
            if y + 1 >= bottom {
                break;
            }
            let Some(row) = self.rows.get(idx) else {
                continue;
            };
            let selected = focused && idx == self.selected;
            paint_line(
                buf,
                area.x,
                y,
                area.width,
                &format_row(row),
                row_style(row, selected, &theme),
                if selected {
                    theme.bg_highlight
                } else {
                    theme.bg_base
                },
            );
            y = y.saturating_add(1);
        }
        if self.show_tasks {
            for task in self.tasks.iter().take(4) {
                if y + 1 >= bottom {
                    break;
                }
                paint_line(
                    buf,
                    area.x,
                    y,
                    area.width,
                    &format!("  · [{}] {}", task.status, task.title),
                    Style::default().fg(theme.text_secondary),
                    theme.bg_base,
                );
                y = y.saturating_add(1);
            }
        }
        if y < bottom {
            paint_line(
                buf,
                area.x,
                y,
                area.width,
                " enter open · x stop · ctrl+t tasks ",
                Style::default().fg(theme.gray),
                theme.bg_base,
            );
        }
    }
}

/// Indices of rows that should paint. Idle rows drop after [`IDLE_COLLAPSE_AFTER`]
/// once every member is idle/done. Pending/working/blocked always stay.
pub fn visible_row_indices(
    rows: &[TeamPanelRow],
    last_all_idle_at: Option<Instant>,
    now: Instant,
) -> Vec<usize> {
    let collapse = last_all_idle_at
        .is_some_and(|t| now.saturating_duration_since(t) >= IDLE_COLLAPSE_AFTER);
    rows.iter()
        .enumerate()
        .filter(|(_, row)| {
            if row.is_lead {
                return false;
            }
            if collapse && row.activity.is_idle_like() && !row.spawn_pending {
                return false;
            }
            true
        })
        .map(|(i, _)| i)
        .collect()
}

fn format_row(row: &TeamPanelRow) -> String {
    let unread = if row.unread > 0 {
        format!(" · {}", row.unread)
    } else {
        String::new()
    };
    format!("  @{}  {}{unread}", row.name, row.activity.label())
}

fn row_style(row: &TeamPanelRow, selected: bool, theme: &Theme) -> Style {
    let fg = match row.activity {
        TeamActivity::Working => theme.accent_running,
        TeamActivity::Blocked => theme.warning,
        TeamActivity::Pending => theme.text_secondary,
        TeamActivity::Done => theme.accent_success,
        TeamActivity::Idle => theme.gray,
    };
    let mut style = Style::default().fg(fg);
    if selected {
        style = style.add_modifier(Modifier::BOLD);
    }
    style
}

fn paint_line(
    buf: &mut Buffer,
    x: u16,
    y: u16,
    width: u16,
    text: &str,
    style: Style,
    bg: ratatui::style::Color,
) {
    if width == 0 {
        return;
    }
    let padded = format!("{text:width$}", width = width as usize);
    let span = Span::styled(padded, style.bg(bg));
    buf.set_span(x, y, &span, width);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(name: &str, activity: TeamActivity, pending: bool) -> TeamPanelRow {
        TeamPanelRow {
            name: name.into(),
            is_lead: name == "lead",
            activity,
            unread: 0,
            session_id: if pending {
                None
            } else {
                Some(format!("sess-{name}"))
            },
            spawn_pending: pending,
        }
    }

    #[test]
    fn desired_height_zero_without_teammates() {
        let mut p = TeamPanel::new();
        p.rows = vec![row("lead", TeamActivity::Idle, false)];
        assert_eq!(p.desired_height(24), 0);
        p.rows.clear();
        assert_eq!(p.desired_height(24), 0);
    }

    #[test]
    fn desired_height_counts_header_rows_footer() {
        let mut p = TeamPanel::new();
        p.rows = vec![
            row("lead", TeamActivity::Idle, false),
            row("reviewer", TeamActivity::Working, false),
        ];
        // header + 1 teammate + footer
        assert_eq!(p.desired_height(24), 3);
    }

    #[test]
    fn idle_rows_stay_while_anyone_is_working() {
        let rows = vec![
            row("lead", TeamActivity::Idle, false),
            row("reviewer", TeamActivity::Working, false),
            row("researcher", TeamActivity::Idle, false),
        ];
        let now = Instant::now();
        let vis = visible_row_indices(&rows, None, now);
        assert_eq!(vis, vec![1, 2]);
    }

    #[test]
    fn idle_rows_collapse_after_30s() {
        let rows = vec![
            row("lead", TeamActivity::Idle, false),
            row("reviewer", TeamActivity::Idle, false),
        ];
        let start = Instant::now();
        let later = start + IDLE_COLLAPSE_AFTER + Duration::from_millis(1);
        let vis = visible_row_indices(&rows, Some(start), later);
        assert!(vis.is_empty(), "idle teammates should hide: {vis:?}");
    }

    #[test]
    fn pending_spawn_never_collapses() {
        let rows = vec![
            row("lead", TeamActivity::Idle, false),
            row("reviewer", TeamActivity::Pending, true),
        ];
        let start = Instant::now();
        let later = start + IDLE_COLLAPSE_AFTER + Duration::from_secs(5);
        let vis = visible_row_indices(&rows, Some(start), later);
        assert_eq!(vis, vec![1]);
    }

    #[test]
    fn arrows_skip_hidden_lead_row() {
        let mut p = TeamPanel::new();
        p.rows = vec![
            row("lead", TeamActivity::Idle, false),
            row("a", TeamActivity::Working, false),
            row("b", TeamActivity::Idle, false),
        ];
        p.selected = 1;
        p.handle_key(&KeyEvent::from(KeyCode::Down));
        assert_eq!(p.selected, 2);
        p.handle_key(&KeyEvent::from(KeyCode::Up));
        assert_eq!(p.selected, 1);
        match p.handle_key(&KeyEvent::from(KeyCode::Enter)) {
            TeamPanelKey::Open { session_id } => assert_eq!(session_id, "sess-a"),
            other => panic!("expected Open, got {other:?}"),
        }
    }

    #[test]
    fn x_interrupts_only_working() {
        let mut p = TeamPanel::new();
        p.rows = vec![
            row("lead", TeamActivity::Idle, false),
            row("a", TeamActivity::Idle, false),
        ];
        p.selected = 1;
        assert_eq!(
            p.handle_key(&KeyEvent::from(KeyCode::Char('x'))),
            TeamPanelKey::Ignored
        );
        p.rows[1].activity = TeamActivity::Working;
        match p.handle_key(&KeyEvent::from(KeyCode::Char('x'))) {
            TeamPanelKey::Interrupt { session_id } => assert_eq!(session_id, "sess-a"),
            other => panic!("expected Interrupt, got {other:?}"),
        }
    }

    #[test]
    fn apply_snapshot_zero_and_one_and_n() {
        let mut p = TeamPanel::new();
        let now = Instant::now();
        p.apply_snapshot(None, now);
        assert!(!p.is_visible());

        p.apply_snapshot(
            Some(TeamSnapshot {
                team_id: "session-deadbeef".into(),
                rows: vec![
                    row("lead", TeamActivity::Idle, false),
                    row("reviewer", TeamActivity::Pending, true),
                ],
                tasks: vec![],
                unread_total: 0,
            }),
            now,
        );
        assert!(p.is_visible());
        assert_eq!(p.desired_height(24), 3);

        p.apply_snapshot(
            Some(TeamSnapshot {
                team_id: "session-deadbeef".into(),
                rows: vec![
                    row("lead", TeamActivity::Idle, false),
                    row("reviewer", TeamActivity::Working, false),
                    row("researcher", TeamActivity::Blocked, false),
                    row("writer", TeamActivity::Idle, false),
                ],
                tasks: vec![],
                unread_total: 2,
            }),
            now,
        );
        assert_eq!(visible_row_indices(&p.rows, p.last_all_idle_at, now).len(), 3);
        assert_eq!(p.unread_total, 2);
    }
}
