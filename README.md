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

| Platform | What to download |
| --- | --- |
| Linux x86_64 | `lynxby-desktop-linux-x86_64.tar.gz`, or the bare `lynxby-desktop-linux-x86_64` binary |
| Windows x86_64 | `lynxby-desktop-windows-x86_64-setup.exe` for the installer, `lynxby-desktop-windows-x86_64.exe` to run without installing |
| macOS arm64 | `lynxby-desktop-macos-arm64.dmg` |

Every asset is listed in `SHA256SUMS`; check a download with
`sha256sum -c SHA256SUMS` on Linux or `shasum -a 256 -c SHA256SUMS` on macOS.

Linux: unpack the tarball or `chmod +x` the binary, then run it. macOS: open the
disk image and drag Lynxby to Applications. Windows: run the setup, or the
standalone executable.

macOS builds are signed with a Developer ID certificate, notarised and stapled, so
Gatekeeper lets them through. Windows builds are signed with Azure Artifact Signing
when the repository is set up for it, though SmartScreen can still warn until a
download has reputation. A fork without the signing configuration still builds: the
log shows a warning and the build is unsigned. Each build prints its platform in the
footer of the sign-in screen.

## Run it

```sh
# local development deployment (the default in a debug build)
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
LYNXBY_DEMO=1 cargo run               # assistant list with sample data
LYNXBY_DEMO=chat cargo run            # a finished conversation, tool calls folded
LYNXBY_DEMO=chat-tools cargo run      # the same with the tool calls unfolded
LYNXBY_DEMO=chat-streaming cargo run  # an answer still on its way
LYNXBY_DEMO=intake cargo run          # an assistant's form, a typed question
LYNXBY_DEMO=intake-choice cargo run   # the same form at a multiple-choice question
LANG=nl_NL.UTF-8 LYNXBY_DEMO=1 cargo run  # in Dutch
```

Debug builds only; nothing is sent anywhere, and the flag does nothing in a
release build.

The window speaks English or Dutch: the person's language from the deployment
once signed in, the system's `LANG` before that. `LYNXBY_REDUCED_MOTION=1` turns
the typing dots and fade-ins off where the GNOME setting is not available.
Manrope ships in `crates/lynxby-desktop/assets/fonts` under the SIL Open Font
License.

## Build and release

- `build.yml` checks formatting, clippy and tests on every push and pull request.
- The same workflow builds and packages a release on Ubuntu, Windows and macOS:
  a tarball and bare binary for Linux, a signed installer and standalone
  executable for Windows, and a signed, notarised disk image for macOS.
- Pushing a tag that starts with `v` publishes those packages as a release, with a
  `SHA256SUMS` file next to them.
- Signing reads its configuration from the repository, and
  [docs/signing-setup.md](docs/signing-setup.md) lists the one-time setup for both.
  macOS: `MACOS_CERT_P12`, `MACOS_CERT_PASSWORD` (a base64 Developer ID Application
  `.p12`), `MACOS_SIGN_IDENTITY` (optional, otherwise read from the `.p12`),
  `APPLE_ID`, `APPLE_TEAM_ID` and `APPLE_APP_PASSWORD`. Windows: the secrets
  `AZURE_TENANT_ID`, `AZURE_CLIENT_ID` and `AZURE_SUBSCRIPTION_ID` for an app
  registration that signs in over OIDC, and the variables
  `ARTIFACT_SIGNING_ENDPOINT`, `ARTIFACT_SIGNING_ACCOUNT` and
  `ARTIFACT_SIGNING_PROFILE` for the Azure Artifact Signing certificate profile. A
  missing value skips that step with a warning, so unsigned builds keep working.

```sh
git tag v0.1.0 && git push origin v0.1.0
```

## Tests

```sh
cargo test                     # unit and integration, no network, no window
cargo test -- --ignored        # needs LYNXBY_TEST_BASE_URL, talks to a deployment
```

```sh
LYNXBY_TEST_BASE_URL=https://your-deployment.example \
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

The client was developed inside the Lynxby platform repository, which is private,
and moved here when it started shipping as an app. The public API clients live in
[lynxby-sdk](https://github.com/LynxbyAI/lynxby-sdk).

## Not built yet

- OS keyring for the token file (today: 0600 in the user data directory).
- Revoking the grant at the deployment when you sign out.
- IME and clipboard paste in the composer.
- Streaming cancellation and retry.
- Auto-update.
