# Lynxby Desktop

The Lynxby application for the desktop, built with [GPUI](https://gpui.rs/). It is a
client and nothing more: sign-in, assistants, chat, tools and usage all come from a
deployment, and every restriction is enforced there.

```
splash ──► sign-in (system browser) ──► assistants ──► chat
```

## Download

Builds for Linux, Windows and macOS are attached to every release:

<https://github.com/LynxbyAI/lynxby-desktop/releases/latest>

The file is the app: unzip it somewhere you can run from and start it. There is no
installer, no code signing and no update feed yet. Each build prints its platform in
the footer of the sign-in screen.

## Run it

```sh
# ddev (the default in a debug build)
cargo run

# production
cargo run -- --env prod

# a specific deployment
cargo run -- --base-url https://staging.example.test
```

The first sign-in registers this build as a native OAuth client and stores the
client id, so later runs reuse it. Tokens are kept per deployment: a ddev session is
never sent to production.

### Linux packages

GPUI links against the system's windowing stack:

```sh
sudo dnf install libxkbcommon-devel libxkbcommon-x11-devel wayland-devel \
    fontconfig-devel freetype-devel vulkan-loader
# Debian/Ubuntu
sudo apt install libxkbcommon-dev libxkbcommon-x11-dev libwayland-dev \
    libfontconfig1-dev libfreetype6-dev libvulkan1
```

If `libxkbcommon-x11.so` is missing but `libxkbcommon-x11.so.0` is installed, the
linker still needs the development symlink — install the `-devel` package rather
than symlinking by hand.

## Sign-in

`POST /oauth/authorize` in the system browser, code returned to a loopback listener
on port 47833, exchanged at `/oauth/token` with PKCE (S256). The password, second
factor and SSO never enter this process. The deployment decides who may do what;
the app only renders what it is given.

After sign-in the app makes three calls:

| Call | What it fills |
| --- | --- |
| `GET /api/v1/me` | who you are, your organisation, the token's scopes |
| `GET /api/v1/assistants` | the assistants your account may use |
| `POST /mcp` (`tools/list`) | the tools this token may call |

A message goes to `POST /api/v1/assistants/{id}/messages/stream` and is rendered
from the deployment's server-sent events (`start`, `thinking`, `tool`, `delta`,
`error`, `done`). Every turn is stored on the server, so usage and audit are the
same as in the browser.

## Work on the screens without a deployment

```sh
LYNXBY_DEMO=1 cargo run      # assistant list with sample data
LYNXBY_DEMO=chat cargo run   # a finished conversation
```

Debug builds only; nothing is sent anywhere, and the flag does nothing in a
release build.

## Build and release

- `build.yml` checks formatting, clippy and tests on every push and pull request.
- The same workflow builds a release binary on Ubuntu, Windows and macOS, and
  attaches one artifact per platform to the run.
- Pushing a tag that starts with `v` publishes those three binaries as a release,
  with a `SHA256SUMS` file next to them.

```sh
git tag v0.1.0 && git push origin v0.1.0
```

## Tests

```sh
cargo test                     # unit and integration, no network, no window
cargo test -- --ignored        # needs LYNXBY_TEST_BASE_URL, talks to a deployment
```

```sh
LYNXBY_TEST_BASE_URL=https://projectsovereign.ddev.site \
  cargo test -p lynxby-client --test ddev -- --ignored --nocapture
```

The `lynxby-client` crate has no UI dependency, which is where the interesting
tests live: PKCE against the RFC vector, the SSE parser under split reads, the
OAuth handshake against a loopback HTTP server, token storage permissions, and the
whole message stream against a mock deployment.

## Layout

```
crates/lynxby-client   protocol: config, PKCE, OAuth, API, SSE, token store
crates/lynxby-desktop  the window: app state, screens, theme, widgets
```

`crates/lynxby-desktop/src/app.rs` owns the state machine and every transition.
The screens only render; they never call the network.

## Where this comes from

The client was developed inside the platform repository and moved here when it
started shipping as an app. The platform, its API and its docs live in
[ProjectSovereign](https://github.com/LynxbyAI/ProjectSovereign).

## Not built yet

- OS keyring for the token file (today: 0600 in the user data directory).
- Revoking the grant at the deployment when you sign out.
- IME and clipboard paste in the composer.
- Streaming cancellation and retry.
- Installers and auto-update.
