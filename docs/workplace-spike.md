# Spike: the desktop app as a workplace

Status: design note for issue #2, 2026-11-19. No production code.
Question: can the window hold a folder, the CLI sessions that work in it, the
shared context they may reach, and one approval queue — while the deployment
stays the only place that decides what anyone may do?

Answer: yes for the window. Three of the pieces it needs do not exist yet: a
per-folder token, a project file API, and the two open MCP issues (#311, #312).
Nothing here adds a local permission check: the app asks, renders, and shows a
403 when the answer is no.

## 1. What the app does today

Two crates: `lynxby-client` (protocol, no GPUI) and `lynxby-desktop` (window).
`app.rs` owns the state machine and every transition; screens only render and
never call the network.

| Screen | Shows | Where it comes from |
| --- | --- | --- |
| Splash | mark, version, host | local (`config.rs`) |
| Login | "Continue in browser" | `/oauth/authorize` opened in the system browser, loopback `127.0.0.1:47833`, PKCE S256, exchanged at `/oauth/token` |
| Assistants | you, organisation, scopes, assistants, tools, 8 recent conversations | `GET /api/v1/me`, `GET /api/v1/assistants`, `POST /mcp` (`tools/list`), `GET /api/v1/sessions` |
| Chat | streamed answer, thinking, tool labels, citations, model, tokens | `POST /api/v1/assistants/{id}/messages/stream` (SSE: `start`, `thinking`, `tool`, `delta`, `error`, `done`); reopening a conversation reads `GET /api/v1/sessions/{id}/messages` |

Every request carries `USER_AGENT = LynxbyDesktop/0.1.0`. Tokens live per
deployment in a 0600 file in the user data directory; refresh happens on 401.
A 403 is rendered and the person stays signed in. There is no model list, no
provider and no API-key field, by rule.

What the app already drops on the floor: `Reply.cost` and `Reply.currency` are
parsed from the `done` event but never shown — the chat footer prints model and
token count only (`screens/chat.rs:368`). `Turn` has no cost or route field.

## 2. The proposed workplace

A new root screen, `Workspace`, with the assistant list one click away. Layout:

```
┌ folders ─────────┬ files ──────────────────┬ session ─────────────────────┐
│ Case 2026-114    │ contracts/msa.pdf  shared│ $ claude                 [x] │
│ Handbook         │ notes/plan.md      local │ > read the msa and …         │
│                  │ drafts/clause.md   proposed                         │
├──────────────────┴─────────────────────────┴──────────────────────────────┤
│ route sonnet-5 · redaction mask · 41 200 tokens · €0,38    approvals (3)  │
└──────────────────────────────────────────────────────────────────────────┘
```

**Open a folder.** `gpui::App::prompt_for_paths(PathPromptOptions { files:
false, directories: true, .. })` exists in gpui 0.2.2 (`app.rs:1116`). The chosen path
is a local working choice, not a permission. The person then links it to a
Lynxby project; the deployment decides whether that project may be opened.

**File status.** Three states, one per row:

| State | Meaning | Source |
| --- | --- | --- |
| local | on disk, not in the project | app compares relative paths with the deployment's list |
| shared | in the project, readable by its members | deployment: project file + document |
| proposed | a write-back waiting in the approval queue (#312) | deployment: approvals |

The status is never computed from a local rule beyond "not in the list". A file
the deployment would not share is listed as local, and an attempt to share it is
a request the deployment refuses.

**Start a session inside the folder.** Four kinds, one shell:

| Kind | What starts | What it costs |
| --- | --- | --- |
| Claude Code | `claude` with `ANTHROPIC_BASE_URL`, `ANTHROPIC_AUTH_TOKEN`, `ANTHROPIC_MODEL` and the Lynxby MCP server | gateway + MCP, both in the ledger |
| Codex | `codex` with the Lynxby MCP server | MCP only: the gateway has no Responses API, so its model traffic is not in the ledger (`docs/harnesses.md`) |
| other MCP CLI | the person's command, same MCP config | depends on whether it uses the gateway |
| Lynxby model | the existing chat, pointed at the folder | assistant turn, fully in the ledger |

The app writes each CLI's config from the base URL and folder token it already
holds, the same variables the `/connect` page documents, and never a provider
key of its own. Pointing the chat at a folder needs one platform change: the
stream endpoint does not accept a `project_id`, although `assistant_messages`
already carries one.

**Footer.** Route, redaction state and ledger cost for the selected session, and
a count that opens the approvals list. Every value comes from the deployment;
where the deployment has no answer the field shows a dash, not an estimate.

**One approvals list.** Runs waiting for a person (`/api/v1/approvals/runs`) and
blocked connector writes (`/api/v1/approvals/actions`) merged into one list,
plus proposals once #312 lands. Approve/reject call the same endpoints the web
screen calls. A 403 on one item is shown on that item; it does not lock the list
— deciding a connector action needs `connector.update` on the person, which a
token's `run.execute` does not grant.

## 3. Starting a CLI inside the app

The split follows the house rule: PTY and screen state in a headless crate (new
`crates/lynxby-terminal`, no GPUI) so it is testable, the widget in
`crates/lynxby-desktop/src/screens/terminal.rs`.

Flow:

1. `native_pty_system().openpty(PtySize { rows, cols, .. })`.
2. `CommandBuilder::new("claude")`, `cwd(folder)`, env vars with the folder
   token; `slave.spawn_command(cmd)`, drop the slave so EOF arrives on exit.
3. A `std::thread` reads the master in 8 KiB chunks and sends them over
   `async-channel` (already a dependency of the desktop crate). Never on GPUI's
   foreground executor: a blocking read there freezes the window.
4. The entity feeds bytes to a terminal grid (`vte`/`alacritty_terminal`
   `Processor`), then `cx.notify()`.
5. Keys go the other way: a focused handler writes bytes to the master; resize
   calls `master.resize`. Paste is bracketed-paste aware.
6. On exit, keep the last screen and the exit status in the tab; kill the child
   when the tab closes, and on app quit so no orphan holds the folder.

Rendering: GPUI 0.2.2 has no terminal widget, but it has what one needs — a
custom `Element` (`element.rs:51`) that paints cell backgrounds with
`Window::paint_quad` (`window.rs:2839`) and glyphs with
`text_system().shape_line` (`text_system.rs:365`) / `paint_glyph`
(`window.rs:2948`). A simpler first cut is one `div` per row of styled spans,
which is slower but needs no custom element.

Candidate crates (checked on crates.io and the upstream repositories, 2026-11-19):

| Crate | Version | Licence | Maintained | Fit |
| --- | --- | --- | --- | --- |
| `portable-pty` | 0.9.0 (2025-02) | MIT | wezterm repo, pty commits 2026-09, releases rare | the PTY layer; Windows ConPTY, macOS and Linux. Recommended |
| `alacritty_terminal` | 0.26.0 (2026-04) | Apache-2.0 | active, commits 2026-11 | grid, scrollback, selection, wide chars. Use `Term` + `Processor`, not its event loop. Recommended |
| `vte` | 0.15.0 (2025-02) | Apache-2.0 OR MIT | stable, low churn | parser only, if we write our own grid |
| `termwiz` | 0.23.3 (2025-03) | MIT | wezterm, same cadence as portable-pty | alternative grid + surface |
| `vt100` | 0.18.1 (2026-10) | MIT | active | small grid, fewer features; good for the first milestone |
| `gpui-terminal` | 0.1.0 (2025-12) | MIT OR Apache-2.0 | one release, ~2.8k downloads, one maintainer | `TerminalView` on gpui ^0.2.2, alacritty_terminal ^0.25.1, portable-pty ^0.9 — the only ready-made widget that matches our pin. Prototype it; be ready to vendor |
| `bezel-terminal` | 0.2.29 (2026-11) | MIT | new, small | builds on `bezel-gpui` ^0.3.25, not gpui 0.2.2 — unusable here today |
| `gpui-libghostty` | 0.4.1-rc.1 (2026-11) | MIT | release candidate | native libghostty build, too much for this |

All licences are compatible with this codebase (MIT/Apache-2.0; the repo itself
has no licence file, which is worth fixing before a distribution argument).

The libc pin stays: `gpui 0.2.2` needs `libc = "=0.2.174"` because of `xattr`.
`portable-pty` and `alacritty_terminal` both take `libc ^0.2`, so the pin holds.

## 4. Platform APIs

`/home/sebastiaan/ProjectSovereign` was read, not edited. Routes below are from
`routes/api.php`; scope names from `App\Models\ApiToken::SCOPES`.

| Need | Today | Exists? |
| --- | --- | --- |
| open a project / list its files | web only: `GET /projects/{project}` in `routes/web.php:705`; no `/api/v1/projects` | no |
| file status per row | `ProjectFile` → `Document` (`scan_status`, collection redaction policy); web presenter returns only `id`, `name`, `collection_id` | data yes, API no |
| token scoped to one folder | `api_tokens` has `organisation_id`, `user_id`, `scopes`, no project; no API route mints a token | no |
| project-scoped assistant turn | `assistant_messages.project_id` exists; `Api\V1\AssistantController` has no `project_id` | data yes, API no |
| MCP `run_task`, `ask_assistant`, `list_parked_runs`, `decide_run` | #311 open; built-ins today are `search_knowledge`, `get_document`, `ask_lynxby`, plus read-only connector actions | no |
| MCP `add_document`, `propose_context` | #312 open; no `propose_context`/`add_document` in `app/` | no |
| approvals list | `GET /api/v1/approvals/runs`, `GET /api/v1/approvals/actions` (`run.read`); decisions at `POST …/approve` and `…/reject` (`run.execute`) | yes, two lists |
| route for a turn | stored on `assistant_messages.routing.route`; the API presenter (`AssistantController::presentMessage`) omits `routing` | data yes, wire no |
| cost for a session | `Reply.cost` on the chat stream; `gateway_requests.cost`/`client_session_id` for CLI traffic; no per-session endpoint, `/api/v1/usage` is organisation totals | partly |
| redaction state | `organisation_settings.message_masking` (default `mask`), `collections.redaction_policy`; not on `/api/v1/me` or any token route | data yes, API no |

What each missing piece has to be:

**Per-folder token.** A child credential, minted by the deployment after the
desktop's OAuth grant asks for it: `POST /api/v1/projects/{id}/tokens` returning
a token with the intersection of the person's scopes and the folder's needs
(`knowledge.read`, `gateway.invoke`, `run.read`/`run.execute`, `assistant.*`).
Schema: a `project_id` (nullable) on `api_tokens`, checked by the same
middleware that checks scopes, so a folder token cannot read another project.
It must expire (hours, not months), be revocable from the web token list, and
never be wider than the grant that asked for it. Do not put a project scope in
the client: the server resolves project → permission on every call. Without a
project-bound token the app would share one broad credential across folders,
which breaks both the ledger and the approval story.

**Project file endpoint.** `GET /api/v1/projects/{id}/files` with `path`,
`document_id`, `scan_status`, `redaction_policy`, `proposal` — the same policy
check as `ProjectController::show`, plus `document.read`. Relative paths are the
deployment's, so the app can compare without guessing.

**Approvals.** The two existing lists are enough for the first milestone. #312
adds a third kind; either a `GET /api/v1/approvals/proposals` or one merged
`GET /api/v1/approvals` with a `kind` field. The merge is display only —
decisions stay per kind, and a 403 on a connector action is expected.

**Footer fields.** `routing` on the message payload (the row already has it) and
`GET /api/v1/usage/session?client_session_id=…` for CLI sessions, or two
response headers on the gateway (`X-Lynxby-Route`, `X-Lynxby-Cost`) that the app
accumulates. Redaction: add `redaction` to `/api/v1/me` (organisation setting)
and read the collection policy from the file endpoint. Do not invent a second
source of truth in the app.

**Project-scoped turns.** Accept `project_id` on
`POST /api/v1/assistants/{id}/messages/stream`, check it with the same policy
`ProjectController` uses, and store it on the message. That is what makes "the
Lynxby model works in this folder" mean the same thing as the web's project
chat.

**#311 and #312** are the same tools the browser uses, exposed over MCP behind
the same scopes, ledger and approval queue. The app only needs them to appear in
`tools/list`; the exposure profile decides that per harness, and an admin can
switch them off, in which case the app says so rather than reimplementing them.

## 5. Plan and risk

| Phase | Work | Size |
| --- | --- | --- |
| 0 | terminal spike in `lynxby-terminal`: PTY + grid + keys, one hard-coded command, no platform work | 3–4 d |
| 1 | `Screen::Workspace`, folder picker, local file rows, session tabs, footer shell with dashes | 8–10 d |
| 2 | platform: project files endpoint + folder token + mint/revoke route + `project_id` on the stream; desktop: shared/proposed status, link folder to project | 6 d platform + 3 d desktop |
| 3 | approvals merge and decisions; proposals when #312 lands (platform) | 3 d desktop + 6–8 d platform |
| 4 | footer from real values: routing on the wire, per-session cost, redaction on `/me` | 3 d platform + 2 d desktop |
| 5 | CLI presets from `/connect`, MCP config written per harness, skill fetch | 2 d |

The desktop side is about four to five weeks; the platform side is roughly three
weeks of work that can run in parallel. Phase 1 needs no platform change and is
the honest demo.

Risks:

- **Terminal emulation is a tarpit.** Selection, scrollback, reflow, IME, wide
  characters, mouse reporting, OSC 8 links. Cut selection and mouse from v1 and
  say so; `gpui-terminal` may save the widget, but it is a young dependency that
  must be vendored if it drifts from gpui 0.2.2.
- **Two clocks.** The gpui 0.2.2 pin and the libc pin are old. Any new crate that
  wants newer `libc` breaks the build before the first screen renders. Phase 0
  proves this, not phase 3.
- **PTY lifetime.** Orphaned `claude`/`codex` processes hold the folder and the
  token file handle. Kill on tab close and on quit, and reap on Windows with a
  job object.
- **The ledger has a hole.** MCP carries no model traffic. A CLI with its own
  provider key is invisible to route, cost and redaction, and the footer must
  say "not routed through Lynxby" rather than show a plausible zero.
- **Folder scoping is the security-critical piece.** A project-bound token that
  is checked in the client is theatre. Until the server resolves project on every
  call, phase 2 should not ship.
- **Path identity.** Symlinks, renames, case-insensitive filesystems and nested
  projects make "is this file shared" ambiguous. Compare the deployment's
  relative paths only, and treat unknown as local.
- **Approval races.** Two windows and the web can decide the same item. Show the
  422 (`not_pending_approval`) and refresh the list.

What the spike did not settle: whether a folder is one project or many (nested
projects), whether the folder token should also cover the `POST /mcp` endpoint
per project, and whether Codex is worth supporting before the gateway serves the
Responses API.
