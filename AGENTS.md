# Working on the desktop app

Read `README.md` first: it has the commands and the shape of the flow.

## Rules of the house

- The deployment is the only place authorisation happens. Do not add a local check
  that decides what a person may do; render what the API returned and show a 403
  when it comes back.
- `crates/lynxby-client` must not depend on GPUI. If a test needs a window, the
  logic is in the wrong crate.
- Every request the app makes carries `USER_AGENT`, so the deployment's audit trail
  can tell desktop traffic apart.
- Never add a provider, base URL or API key field. This app has one deployment.
- Keep the palette in `theme.rs` derived from `resources/css/app.css`; do not
  invent colours in a screen.

## Adding a screen

1. Add a variant to `Screen` in `app.rs`.
2. Add a transition method there (`open_x`, `back_...`), and unit-test its state
   change where it is pure.
3. Add `screens/x.rs` with `pub fn render(app: &mut LynxbyApp, cx: &mut Context<LynxbyApp>) -> Div`.
4. Wire it in `Render for LynxbyApp`.

## Testing

- Pure logic goes in the client crate with unit tests next to it.
- Anything with a request goes in `crates/lynxby-client/tests/flow.rs` against the
  mock server; add a case per error shape you handle.
- `LYNXBY_TEST_BASE_URL=... cargo test -- --ignored` is the smoke test against a
  real deployment. It must stay read-only.

## Gotchas

- `gpui` 0.2.2 needs `libc = "=0.2.174"`; a newer libc breaks its `xattr`
  dependency at build time. Remove the pin only after checking a newer GPUI.
- Elements need an `.id()` before `.on_click()` and before `.overflow_y_scroll()`.
- The key handler on the composer ignores platform/control chords so the window
  keeps its own shortcuts.
