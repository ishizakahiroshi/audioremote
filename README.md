# audioremote

Switch your **Windows 11 host's default audio output device** from any browser on the LAN.
No more walking back to the host to change the output from Nest Hub Max to wired earphones
during a meeting — do it from a Hyper-V guest, WSL2, your phone, or another PC.

> Status: **v0.1.0 released** (2026-07-31) on npm, crates.io and GitHub Releases.

---

## How it works

```
Browser / phone / other PC
        ↓ HTTP(S)
audioremote server (Rust, runs as your logon user on the Windows 11 host)
        ↓ windows crate (COM)
Windows Core Audio (IMMDeviceEnumerator / IPolicyConfig)
        ↓
Physical devices (Nest Hub Max / wired earphones / headphones / …)
```

- The server runs in **your physical console session** (not as a SYSTEM service, and not inside an RDP session — audio endpoints belong to the interactive user's physical session; from an RDP session you only see the virtual "Remote Audio" endpoint).
- Default bind is **`0.0.0.0:17650`** — exposed to the LAN out of the box, because controlling the host from another machine is the whole point. Non-loopback clients require a bearer token; loopback (the host itself) is bypassed. Lock it down to `127.0.0.1` with `audioremote setup` if you don't need remote control.
- Console / Multimedia / Communications default endpoints are always **switched together** (otherwise meeting apps still route to the old device via the Communications default).
- The Web UI controls the **master volume and mute state** of the current default Multimedia output. Volume is per output device, so it follows the selected endpoint after a device switch.
- Switching is done by **device ID**, not display name (display names change on reconnect).

## Supported platforms

| Side | Support (v0.1) |
|---|---|
| Server (host) | **Windows 11 only.** Windows 10 is not officially supported. macOS / Linux are out of scope. |
| Client (browser) | Any modern browser on any OS. The built-in Web UI is served by the host binary. |

## Install

**npm** (primary — no Rust toolchain needed):

```powershell
npm i -g audioremote
# or run it once without installing:
npx audioremote
```

The platform-specific Rust binary ships via `optionalDependencies` (esbuild / Biome style), so
`npm i` pulls the right executable for your machine. Package-manager installs skip Mark of the Web,
so SmartScreen warnings are avoided.

**GitHub Releases** (Node-free): download `audioremote-win32-x64.zip` from the
[latest release](https://github.com/ishizakahiroshi/audioremote/releases/latest), verify it against
the published `SHA256SUMS.txt`, and run the extracted `audioremote.exe`.

**crates.io** (builds from source, needs Rust 1.85+):

```powershell
cargo install audioremote
```

All three are live as of v0.1.0. The binary is **unsigned** — see Security posture for why the npm
channel is the recommended path.

## Build from source (developer)

```powershell
# On Windows 11 with Rust 1.85 or newer (see `rust-version` in Cargo.toml)
cargo build            # dev build
cargo run              # starts the local HTTP server
cargo test
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --all
npm test               # npm launcher (bin/audioremote.js)
```

`cargo build --release` produces `target/release/audioremote.exe`.

## Command line

```text
audioremote                     start the HTTP server (default)
audioremote serve --no-open     start without opening a browser (used by autostart)
audioremote setup               interactive config wizard (bind / token / sort / port)
audioremote list                list playback endpoints + current defaults
audioremote set <id>            switch the default output device
audioremote share               print the LAN URLs with the token in full
audioremote token list          list tokens (masked)
audioremote token list --show   list tokens in full
audioremote token add <name>    issue a new named token
audioremote token revoke <name|token>
```

`token add` and `token revoke` take effect on a **running** server within a
second — no restart. Everything else in `config.toml` (bind, port,
`allowed_networks`, `device_sort`) is read once at startup.

## Volume and mute

Open the built-in Web UI from the host or a LAN client. The master-volume panel
uses the current default **Multimedia** render endpoint and provides a 0–100%
slider plus mute/unmute. Changes made in Windows are picked up by the Web UI's
three-second refresh, and switching the output device refreshes the panel for
the new endpoint.

The same state is available through the authenticated HTTP API:

```text
GET  /api/volume
POST /api/volume   {"level": 0.5}
POST /api/volume   {"muted": true}
```

`level` is a finite scalar from `0.0` to `1.0`; invalid requests return HTTP
400. Only the fields present in the body are applied, so a mute-only request
cannot clobber a level someone changed in Windows a moment earlier. The
bearer-token, Host-header, allowlist and same-origin checks all apply.

## Start at logon (v0.1 minimal autostart)

Register the current executable in the per-user HKCU Run key:

```powershell
.\target\release\audioremote.exe --install-autostart
```

Remove only AudioRemote's own Run value with:

```powershell
.\target\release\audioremote.exe --uninstall-autostart
```

The registered command is the quoted absolute exe path followed by
`serve --no-open`, so signing in starts the server without opening a browser.
This v0.1 form does not add firewall rules, request UAC elevation, or hide the
console window; a console may remain visible. If the exe is moved, run the
install command again.

## Roadmap

- **v0.1** — Host server + HTTP API + built-in Web UI, including device switching,
  master volume/mute, and minimal per-user autostart (this milestone).
- **v0.2** — Guest-resident companion app on Windows: tray-icon popup + global hotkey for the daily meeting toggle (e.g. Nest Hub Max ⇄ wired earphones). Same HTTP API, different client.
- **v0.3+** — Additional entry points (PWA / app window) as needed.

All clients are thin HTTP clients over the same API. The core architecture (host-resident server + HTTP API) does not change between versions.

## Configuration (v0.1)

Config lives at `%APPDATA%\audioremote\config.toml` (created automatically on first run). See the UX mockup for the concrete layout; the shape is roughly:

```toml
[server]
bind = "0.0.0.0"     # LAN-exposed by default; "127.0.0.1" to lock to this host
port = 17650
allowed_networks = []   # optional allowlist: ["203.0.113.0/24", "198.51.100.5"]; empty = any

[auth]
require_token = true

# One or more named bearer tokens (first run generates a "default").
# Manage with `audioremote token add|revoke|list`.
[[auth.tokens]]
name = "default"
token = "ar_live_..."   # auto-generated on first run
revoked = false

[audio]
device_sort = "state"   # "state" | "name" | "recent"
```

Notes on hand-editing:

- `bind` accepts an IPv4/IPv6 literal or `localhost`; host names are not
  resolved and `port = 0` is refused. An unusable value is reported at startup
  with the reason instead of failing obscurely.
- `allowed_networks` accepts CIDR (`"203.0.113.0/24"`) or a bare address
  (`"203.0.113.20"`, treated as a single host). Entries that parse as neither
  match nothing — the startup banner names them so a typo does not read as "the
  server ignores my LAN".
- Console / Multimedia / Communications are **always** switched together; there
  is no setting for it (see Non-goals).

Device usage history (for `device_sort = "recent"`) is stored separately in `%APPDATA%\audioremote\history.toml` so editing config by hand does not clobber it.

## Security posture

- **Exposed to the LAN by default** (`bind = "0.0.0.0"`). The Windows Firewall prompt on first run is the outer gate; the bearer token is the inner gate. Lock down with `audioremote setup` (bind `127.0.0.1`) if you don't want remote control.
- Bearer token authentication required for every **non-loopback** client on all API endpoints; loopback (the host itself) is bypassed. Tokens are named and individually revocable — `audioremote token add|revoke|list`.
- **Revocation is immediate.** The running server re-reads the token set when `config.toml` changes (checked at most once a second), so `token revoke` stops a leaked token without a restart. Writes are atomic, so the server never reads a half-saved file.
- **DNS-rebinding guard**: a request is accepted only when its `Host` header matches loopback or a current LAN IP, so a malicious page whose DNS re-resolves to `127.0.0.1` cannot reach the API. Applies to the Web UI assets as well, not just the API.
- Optional **allowlist** (`allowed_networks`) refuses non-loopback source IPs outside the listed networks before token checking.
- **Cross-origin writes are refused.** Because loopback skips the token, any web page could otherwise `fetch()` a device switch at `127.0.0.1` while you browse. State-changing requests must carry `Sec-Fetch-Site: same-origin`/`none` and, when an `Origin` is present, an authority matching the request's `Host`. Non-browser clients (curl, scripts) send neither header and are unaffected. No CORS handler is installed, so cross-origin **reads** stay blocked by the browser.
- **Framing is refused** (`Content-Security-Policy: frame-ancestors 'none'` + `X-Frame-Options: DENY`), so the token-free loopback UI cannot be used for clickjacking. Every response also carries `X-Content-Type-Options: nosniff` and `Referrer-Policy: no-referrer`.
- **Tokens are masked in console output.** The startup banner prints full share URLs only on the very first run; afterwards the token is masked and `audioremote share` prints it on demand. `token list` masks by default (`--show` to reveal). This keeps live credentials out of scrollback, screen shares and redirected logs — the autostart entry re-prints the banner at every logon.
- When bound to LAN, the guest UI shows a **"LAN exposed"** badge as a reminder.
- No unsigned exe direct-download flow is recommended for end users; use the npm channel to avoid SmartScreen prompts.
- The v0.1 autostart command does not modify RDP settings, audio drivers, or Windows Firewall rules.

### Plain HTTP, and what that costs

v0.1 speaks **HTTP, not HTTPS**. On a LAN segment you control that is a
considered trade, not an oversight — but be clear about it: the bearer token
travels in a header in the clear, so anyone able to sniff the segment (an
untrusted Wi-Fi AP, an ARP-spoofing device) can capture and replay it until you
revoke it. Accordingly:

- Run it on a **trusted private LAN** only. Choose "Private networks" at the
  Windows Firewall prompt, never "Public".
- Narrow the reachable set with `allowed_networks`, and issue **one token per
  device** so a single leak can be revoked without disturbing the others.
- If you need transport encryption, put a TLS reverse proxy in front and take the
  server off the LAN entirely: `audioremote setup` → bind `127.0.0.1`, then have
  the proxy (Caddy, nginx, IIS ARR) terminate TLS on the same machine and forward
  to `http://127.0.0.1:17650`. Send the **upstream** authority as `Host` —
  `proxy_set_header Host 127.0.0.1:17650;` in nginx, `header_up Host {upstream_hostport}`
  in Caddy — because the rebinding guard only accepts loopback and this host's own
  LAN IPs, not your proxy's hostname. Writes still work: modern browsers send
  `Sec-Fetch-Site: same-origin`, which is checked instead of comparing `Origin` to
  `Host`. A browser old enough to omit that header would need `Origin` rewritten to
  match as well; a first-class "trusted hostname" setting is deferred to v0.2.
- Automatic HTTPS provisioning (mkcert, self-signed helpers) stays out of scope
  for v0.1; see Non-goals.

## Non-goals (v0.1)

- Tray icon / global hotkeys (that's v0.2's guest companion).
- Per-role (Console / Multimedia / Communications) individual switching UI.
- macOS / Linux server implementations.
- Windows 10 official support.
- Automatic HTTPS provisioning, mkcert integration, self-signed helpers.
- Code signing / winget submission.

## Project layout

```
audioremote/
├── src/                        Rust sources (binary crate)
├── web/                        embedded Web UI (vanilla JS, no build step)
├── bin/audioremote.js          npm launcher (resolves + runs the native binary)
├── npm/platforms/              per-platform npm packages carrying the .exe
├── test/                       node:test suite for the npm launcher
├── Cargo.toml                  package definition
├── CLAUDE.md / AGENTS.md       AI entry points
├── LICENSE                     MIT
├── scripts/                    secrets-scan + hook installer
├── .githooks/                  layer 2 pre-commit
├── .github/workflows/          CI (validate.yml) + release + secrets-scan
└── docs/
    └── local/                  plan / recap / bugfix / mockup (gitignored — local-only)
```

## License

MIT — see [LICENSE](LICENSE). Copyright (c) 2026 Hiroshi Ishizaka (ishizakahiroshi).
