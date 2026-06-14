# AGENTS.md — Usage Monitor project directives

> Read this every session before touching code. This file is the operating
> rules for agents working in this repository.

## What this project is

Usage Monitor is a Rust workspace that monitors AI provider usage from the
terminal and Linux desktop widgets.

- `usage-monitor-core` owns provider integrations, config, data models, and
  fetch logic.
- `usage-monitor-cli` exposes the command-line interface and widget JSON
  payloads.
- `widgets/` contains KDE Plasma 6 and Waybar integrations.
- `docs/` and `releases/` are part of the product surface and must stay in sync
  with user-visible behavior.

## Stack

- **Runtime:** Rust edition 2024, workspace resolver 3.
- **Async/HTTP:** `tokio`, `reqwest`.
- **CLI:** `clap`.
- **Serialization/config:** `serde`, `serde_json`, `toml`.
- **Desktop widgets:** KDE Plasma 6 QML + Python helper; Waybar JSON wrapper.
- **Quality:** rustfmt, Clippy, Rust tests, Ruff, Python widget tests, optional
  `qmllint` when installed.

## Repository layout

```text
usage-monitor-core/     Core library: models, providers, registry, config
usage-monitor-cli/      Command-line interface
widgets/                KDE Plasma and Waybar integrations
docs/                   User/developer documentation
releases/               Per-version release notes
.githooks/              Versioned Git hooks
tests/                  Integration test assets
assets/                 Project artwork used by documentation/README
```

## Workflow rules

1. **Keep changes scoped.** Do not refactor unrelated provider/widget code while
   doing a documentation, release, or CI-only task.
2. **Tests before claiming done.** Run the relevant subset; for release/CI work,
   run the CI-equivalent checks:

   ```bash
   cargo fmt -p usage-monitor-cli --check
   cargo clippy --workspace --all-targets -- -D warnings
   cargo test --workspace
   ruff check widgets
   python -m unittest discover -s widgets -p 'test_*.py'
   ```

3. **When changing user-visible behavior, update docs.** README, `docs/`,
   `CHANGELOG.md`, and `releases/` must match CLI/widget behavior.
4. **Release bumps touch every version surface.** For a version bump, update:
   `Cargo.toml`, `Cargo.lock`, `widgets/kde/package/metadata.json`,
   `widgets/kde/package/contents/code/usage_monitor_kde.py`, `CHANGELOG.md`,
   and `releases/vX.Y.Z.md`.
5. **Provider auth stays explicit and safe.** Never log secrets, tokens, cookies,
   API keys, OAuth credentials, or raw auth files. Tests must use mock data.
6. **KDE icon rule.** The plasmoid metadata/About/panel icon uses Plasma's
   stock `utilities-system-monitor` icon unless packaging is changed to install a
   real icon-theme asset. The project logo lives in `assets/` for docs/README.
7. **Do not invent provider behavior.** If an API shape is unclear, add a focused
   parser test with representative JSON/protobuf/HTML before changing fetch
   logic.
8. **Prefer small, reviewable commits.** Inspect `git status`, `git diff`, and
   recent commits before committing. Commit only intended files.

## Quick commands

```bash
# Build
cargo build --release

# Install CLI locally
cargo install --path usage-monitor-cli

# Run all Rust tests
cargo test --workspace

# Widget Python tests
python -m unittest discover -s widgets -p 'test_*.py'

# Local quality gate
.githooks/pre-commit

# KDE development install/update
kpackagetool6 --type Plasma/Applet --install widgets/kde/package
kpackagetool6 --type Plasma/Applet --upgrade widgets/kde/package
```

## Project-specific gotchas

- `fetch` can return one block per provider account; preserve multi-account
  behavior in CLI output and widget payloads.
- Machine-readable output (`--json`, widget payloads) should stay stable when
  improving human-readable text.
- Browser-cookie providers require explicit config/env credentials; do not add
  browser auto-import without documenting and testing the security model.
- KDE helper code is intentionally testable without a running Plasma session;
  keep logic in Python helpers where it can be unit-tested.
- CI currently mirrors the commands in `.github/workflows/ci.yml`; if changing
  the workflow, update this file and `docs/quality.md` when needed.

<!-- ai-memory:start -->
## Long-term memory (ai-memory)

This project uses [ai-memory](https://github.com/akitaonrails/ai-memory)
for cross-session continuity.

**Default to the current project — always.** Every ai-memory tool
auto-scopes to the project resolved from your session's working
directory. **Do NOT pass `project` or `cwd` arguments unless the user
explicitly references a *different* project by name** (e.g. "what did we
decide in the `other-app` project?"). Phrases like "this project",
"here", "we", "our work", "where did we leave off" all mean the *current*
project — call the tool with no scoping args. If the user asks about a
handoff and the SessionStart auto-fetched block is already in your
context, just answer from it; do not re-call the tool to "find it again"
in another project.

**Lifecycle hooks already capture every prompt + tool call
automatically.** You never need to manually write routine notes; the
SessionStart hook auto-fetches pending handoffs, and on session end
ai-memory writes a session-summary page and a handoff.
LLM consolidation (compiling observations into topical wiki pages) runs
on PreCompact, on demand via `memory_consolidate`, and at session end
only when the server sets `AI_MEMORY_CONSOLIDATE_ON_SESSION_END`. Only
write a durable wiki page when the user explicitly asks to remember or
annotate something permanently.

### When to reach for each tool

The user can express any of the intents below in plain English —
match the intent to the tool. They do not need to name the tool.

| User says / situation | Tool |
|---|---|
| "have we discussed X?" / "search memory for Y" / before proposing architecture | `memory_query` (current project; `scopes` for named siblings; `global=true` to search every project) |
| "what's been going on" / "show recent activity" (light) | `memory_recent` |
| "is ai-memory healthy?" / "how big is the wiki?" | `memory_status` |
| "give me the stats" / structured snapshot for the agent to consume | `memory_briefing` (read-only; never creates handoffs) |
| "catch me up" / "I've been away" / "what's important right now?" / open-ended exploration | `memory_explore` |
| "where did we leave off?" — and you see a `📥 ai-memory: pending handoff` block in your context | already done — answer from that block; do NOT re-call `memory_handoff_accept` |
| "where did we leave off?" — and no such block is visible | `memory_handoff_accept` (rare; the SessionStart hook usually got there first) |
| "save context for the next session" / wrapping up / ending this session | `memory_handoff_begin` (session-end only; do **not** use for status/briefing; single-use handoff; terse summary; put detail in `open_questions` + `next_steps` bullets) |
| "discard that handoff" / "I created a handoff by mistake" | `memory_handoff_cancel` (requires exact `handoff_id` from `memory_handoff_begin`; marks it expired before the next session sees it) |
| "consolidate this session" / "compile what we learned" | `memory_consolidate` |
| "remember this permanently" / "save a note" / "add an annotation" / durable project knowledge | `memory_write_page` (write a wiki page; do **not** use handoff for permanent notes; put the title as a `# H1` on the first line of `body` and omit the `title` arg) |
| "read the page about X" / "show me the full content of Y" / "open the page on Z" | `memory_read_page` |
| "delete the page X" / "remove that note" | `memory_delete_page` |
| "audit the wiki" / "find contradictions" / "what rules should we add?" | `memory_lint` |
| "prune old pages" / "memory cleanup" | `memory_forget_sweep` |

`memory_explore` is the right default for the "I want to know what's
going on" use case — it returns a prose digest whose verbosity
scales automatically to how long it's been since the last activity
(< 1 h → one line; > 30 days → full catchup).

### When the current project comes up empty — broaden the search

`memory_query` searches only the **current** project by default. If a
search comes back empty or thin, the knowledge may live in a **sibling
project** — shared `infra`, `ops`, or a related app. Don't conclude
"we never recorded it" after a single project misses; broaden instead:

- **Know which projects to check?** Re-run with explicit `scopes`, e.g.
  `scopes: [{ "workspace": "default", "project": "infra" }]`.
- **Don't know where it lives?** Pass `global=true` to search every
  project in every workspace at once. Each hit is annotated with its
  workspace + project so you can tell where it came from. `global=true`
  cannot be combined with `scopes`/`project`/`workspace`.

`memory_query` returns **snippets, not full page bodies** — an empty or
short snippet does **not** mean the page is empty. To read the whole
page, use `memory_read_page`.

### When you write a project rule, write it here

If you're about to write a durable project rule ("always X", "never
Y", "all PRs must …"), this rules file is where it belongs.

### Refreshing this snippet

This block is maintained by ai-memory. Ask "refresh the ai-memory routing in
this project" to replace the block bracketed by the ai-memory markers without
disturbing the rest of the file.
<!-- ai-memory:end -->
