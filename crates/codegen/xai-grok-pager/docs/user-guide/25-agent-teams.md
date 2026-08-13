# Agent Teams (experimental)

An **agent team** is a lead session plus named teammates that share a mailbox and a task list. Teammates are **top-level pager sessions in the same process**, not [`spawn_subagent`](16-subagents.md) children. They appear in a panel **above the lead prompt**.

This is off by default. It is not the Agent [Dashboard](23-dashboard.md) (a fleet switcher), and it is not a tmux/Orca split.

---

## Enable

Environment variable (highest priority; `0` / `false` wins over config):

```bash
export GROK_EXPERIMENTAL_AGENT_TEAMS=1
```

Or in `~/.grok/config.toml`:

```toml
[features]
agent_teams = true
```

Restart the pager after changing the flag. Official `~/.grok/bin/grok` is unchanged unless you install a local build.

---

## How it works

1. The **lead** (the session that first calls `spawn_teammate`) gets a team directory under `$GROK_HOME/teams/session-<8 hex chars>/`.
2. Each teammate is a full top-level session: same worktree, own transcript, own prompt.
3. Members talk with `send_message` (mailbox on disk). The pager injects unread mail as a user turn tagged `[from teammate <name>, not the human]`.
4. Shared work uses `team_task_create` / `team_task_claim` / `team_task_complete`.
5. When a teammate goes idle — or a turn fails — the pager writes an `idle` / `failed` mailbox entry to the **lead**. That is not an Orca `worker_done` and not a hook `teammate` type.

Team tools are listed only on the **primary** session when the flag is on. `spawn_subagent` children never see them.

---

## Panel and keys

The panel is visible only on the lead, and only after at least one teammate exists (or a spawn is in flight).

| Key | Action |
|---|---|
| ↑ / ↓ | Select a teammate (the lead row is hidden) |
| Enter | Open that teammate's session |
| `x` | Interrupt a **working** teammate |
| Click | Focus the panel |
| Tab from scrollback | Focus the panel when it is visible |
| Esc / Tab from the panel | Return to the prompt |
| Ctrl+T (panel focused) | Toggle the shared task list |

Idle teammate rows stay visible while anyone is working. After the whole team is idle, idle rows hide after 30 seconds. Pending spawn rows never hide.

Row states: `pending` · `working` · `blocked` (permission or question) · `idle` · `done`.

---

## `/team`

`/team` (optional `status`) prints members, session binding, unread counts, and shared tasks. Hidden unless the flag is on. Alias-free. Session-scoped.

```
/team
/team status
/team help
```

---

## Tools

| Tool | Who | Effect |
|---|---|---|
| `spawn_teammate` | lead only | Record a named member. The pager starts a top-level session. |
| `send_message` | any member | Append to the recipient inbox (`text`, `plan_approval`, `shutdown`, `task_update`, `idle`, `failed`) |
| `team_task_create` | lead | Insert a pending task |
| `team_task_claim` | any | Exclusive claim; fails if blocked or taken |
| `team_task_complete` | assignee or lead | Mark done; unblock dependents |
| `team_status` | any | Snapshot of members, tasks, unread |

Do **not** also call `spawn_subagent` for a teammate. Nested teams are rejected.

---

## Not a team

| Surface | Why it is different |
|---|---|
| [Dashboard](23-dashboard.md) | Lists every top-level session. No membership, mailbox, or shared tasks. |
| [Subagents](16-subagents.md) | Depth-1 children. Report only to the parent. No peer send. |
| `/workflows` | Parent-owned Rhai DAG. |
| Hooks | `backgroundTasks[].type` is `shell` / `monitor` / `subagent`. There is no `teammate` hook type. |

To mix Grok with Codex or Claude in split panes that survive closing the lid, run this pager **inside** Herdr or Orca. This feature does not own PTYs.

---

## Limits (v1)

- One team per lead session. No nested teams, no promoting a teammate to lead.
- Same worktree. No git worktree isolation.
- Grok-only teammates.
- In-process teammates are not resumed after the leader process exits.
- A teammate message cannot approve a permission or plan.
