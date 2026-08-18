# Agent Teams closeout and landing proposal

Date: 2026-08-14  
Branch: `feat/agent-teams-panel`  
Tree: `/Users/gilje/src/grok-build`  
Fork: https://github.com/rp0927/grok-build  
Upstream: https://github.com/xai-org/grok-build (do **not** open a PR)  
Design: [agent-teams.md](agent-teams.md)  
User-guide: `crates/codegen/xai-grok-pager/docs/user-guide/25-agent-teams.md`

This note is the closeout judgment for the already-implemented P0–P3 work
and the landing proposal for a later reader. It does **not** change product
defaults. The three open questions stay undecided.

---

## Done vs deferred

### Done (shipped on this branch, flag off by default)

| Phase | What landed | Tip of the work |
|---|---|---|
| P0 | Feature flag + on-disk team store (config, mailbox, tasks, claim lock) | `1ecc3ce` |
| Benchmark | Claude / OpenCode / Codex / Grok 1.0.3 / Herdr / Orca / Squad matrix | `a1271ec` |
| P1 | Tools `spawn_teammate`, `send_message`, `team_task_*`, `team_status`. Primary + flag only. Hidden from `spawn_subagent`. Pending member is `session_id: None` + `spawn_prompt`. | `57e1a56` |
| P2 | In-TUI panel above the lead prompt. ↑↓ / Enter / `x`. Pending rows become **top-level** pager sessions (not subagents). Mailbox injects tagged user turns. Idle rows collapse after 30s. | `0236631` |
| P3 | Lead mailbox `idle` / `failed` notify. `/team` slash (hidden unless flag on). User-guide page. Ignored pty case: flag off ⇒ no panel chrome. | `f3a2d56` |

Constraints that were kept:

- Official `~/.grok/bin/grok` was not overwritten.
- No PR against `xai-org/grok-build` (`CONTRIBUTING.md` rejects external PRs).
- Teammates are same-worktree top-level sessions, not `spawn_subagent`.
- Herdr / Orca were not cloned.

### Deferred (not in this closeout)

- Live TUI walkthrough of `spawn_teammate` (interactive pager cannot be driven here).
- Ignored pty e2e with flag **on** + a real spawn loop.
- Resume of in-process teammates after the leader process exits.
- Mixed Grok + Codex + Claude teammates inside this TUI.
- Nested teams / promoting a teammate to lead.
- Git worktree isolation for teammates.

### Three open product questions — **undecided**

Do **not** encode a choice in code until the user picks one.

1. **Dashboard visibility.** Keep teammate sessions as ordinary top-level
   dashboard rows (current behavior: Dashboard lists every top-level
   session), **or** hide members that already appear in the team panel.
   Options: (A) leave Dashboard unchanged; (B) filter team members from
   the roster; (C) badge them but keep the row.
2. **Max teammates.** No hard cap is implemented. Claude’s in-process
   teams often start at 3–5; the 42workspace Orca `/agent-team` analog
   caps at 3. Options: (A) leave uncapped; (B) cap at 3; (C) cap at 5.
3. **How to run the fork binary.** Options: (A) keep using official
   `~/.grok/bin/grok` and never install over it (current policy);
   (B) build `target/release/xai-grok-pager` and expose it as
   `~/.local/bin/grok-dev` only; (C) later, if asked, replace the
   official binary. **(C) is out of scope unless explicitly requested.**

---

## Landing proposal

### Where the code is

```
/Users/gilje/src/grok-build          # clone
branch: feat/agent-teams-panel      # 2 commits ahead of fork/feat/agent-teams-panel as of 2026-08-14
remote fork:  https://github.com/rp0927/grok-build.git
remote origin: https://github.com/xai-org/grok-build.git   # READ only
```

Push, if wanted later: `git push fork feat/agent-teams-panel`.  
Do **not** `git push origin` and do **not** open a PR on `xai-org/grok-build`.

### How to enable (read-only until a local build is used)

Environment (wins; `0` / `false` disables even if toml is true):

```bash
export GROK_EXPERIMENTAL_AGENT_TEAMS=1
```

Or `~/.grok/config.toml`:

```toml
[features]
agent_teams = true
```

The official 1.0.3 binary does **not** contain this branch. Enabling the
flag on installed `grok` is a no-op until you run a binary built from
`feat/agent-teams-panel`.

### How to run a local build without touching official `grok`

```bash
cd /Users/gilje/src/grok-build
cargo build -p xai-grok-pager-bin --release
# run in place — do not cp onto ~/.grok/bin/grok
GROK_EXPERIMENTAL_AGENT_TEAMS=1 ./target/release/xai-grok-pager
```

Optional later (only if option 3B is chosen): symlink that binary to
`~/.local/bin/grok-dev`. Do not replace `~/.grok/bin/grok`.

### In-TUI team vs Orca `/agent-team`

| | This fork (in-TUI) | 42workspace `/agent-team` |
|---|---|---|
| Layer | In-process team protocol inside **one** Grok pager | Host multiplexer: Orca splits terminals |
| Who runs | Grok lead + Grok teammates (same process) | Grok and/or Codex in separate panes |
| Panel | Native band above the lead prompt | Desktop/terminal split, not a Grok widget |
| Mailbox | `$GROK_HOME/teams/.../inboxes` + `send_message` | Orca `orchestration send` / inbox |
| When to use | One Grok leader, peer teammates, no extra host | Mix CLIs, see all panes, survive closing one TUI |

They compose: you can still run this fork **inside** an Orca or Herdr pane
if you want mixed-provider split. This feature does not replace `/agent-team`.

The 42workspace `/agent-team` skill file was **not** present in the tree
used for this closeout, so it was not edited. If it is restored, add one
sentence pointing here: in-TUI teams live on `feat/agent-teams-panel`; the
skill remains the Orca split path.

### Suggested next landing steps (no code until chosen)

1. User picks A/B/C on the three questions above.
2. Optional: `git push fork feat/agent-teams-panel` (fork only).
3. Optional: release build + `grok-dev` shim (question 3B only).
4. First live check: flag on, one `spawn_teammate`, confirm the panel row
   and Enter-to-open. Not gated in this closeout.

---

## Tests that remain the contract

From `/Users/gilje/src/grok-build`:

```bash
cargo test -p xai-grok-tools --lib team
cargo test -p xai-grok-pager --lib team_
cargo test -p xai-grok-pager --lib commands::team
```

Do not treat ignored pty e2e as a pass gate.
