# Flint

A native GTK4 command launcher for Linux (Wayland / Hyprland). Alt+Space.

Flint 0.4 is a public, local-first release. It includes the launcher and productivity features listed below; it does not claim compatibility with Raycast's proprietary store or extension runtime.

![Flint](share/flint.png)

Flint stays resident: the first launch keeps a daemon so clipboard history, notes, dictation, and Ask AI stay warm.

Flint opens as a normal desktop window. Under Hyprland it floats centered at
980×720 on the first frame (`no_anim` so it does not tile large then shrink),
still resizable, with a title bar; closing the window hides it while the
resident process remains warm. Copy [`share/hyprland.conf`](share/hyprland.conf)
for the compositor rules, or let Flint install the same float rule at daemon start.

## Install

Build dependencies are a Rust toolchain, GTK4, GLib, Pango, Cairo, Graphene,
and `pkg-config`. Runtime integrations use `curl`, `xdg-open`, and optionally
`secret-tool` (recommended for keyring-backed AI credentials).

```sh
git clone https://github.com/aco-linux/flint.git
cd flint
make install
```

That puts `flint` in `~/.local/bin` and a desktop entry under `~/.local/share/applications`. To refresh an existing checkout from origin, rebuild, install, and restart the resident daemon:

```sh
make update
```

`make update REF=origin/main` tracks a different ref. Then bind it:

```
exec-once = flint --daemon
bind = ALT, SPACE, exec, flint
bind = SUPER SHIFT, R, exec, flint --windows
bind = SUPER SHIFT, F, exec, flint --files
```

On Omarchy, Super+Space stays the system menu. Bind Flint to Alt+Space in `~/.config/hypr/bindings.lua`. A full snippet is in [`share/hyprland.conf`](share/hyprland.conf).
When upgrading from 0.1, replace the old `dev.flint.Launcher` window-rule class
with the release ID `dev.flint.launcher` so the centered floating rule still applies.

## Modes

| Prefix | Mode | Also |
| --- | --- | --- |
| _(empty)_ | Apps, files, calc, extensions | Alt+Space |
| `file` / `f` / `find` | Search Files (type, name, or path) | `--files` |
| `win` | Window switcher | `--windows` |
| `clip` | Clipboard history | `--clipboard` |
| `;` / `snip` | Snippets | `--snippets` |
| `note` | Notes | `--notes` |
| `?` / `ask` | Ask AI | `--ask` |
| `voice` | Dictation | `--voice` |
| `set` | Settings | `--settings` |
| `store` | Store | `--store` |
| `link` / `links` | Quicklinks | type `+name url` to save |
| `calc` / `=` | Calculator + history | dates, percents, math |

Type `+keyword` in snippets to save the clipboard. Type `+title` in notes to create one. Prefix `>` to run a command, `$` to run it in a terminal.

Root search is intent-aware, closer to Raycast than a fixed 12-row list:

- Type `we` and Flint already means weather: it geolocates you and fills in the current conditions.
- Apps, calc, commands, and windows paint on the same keystroke. Windows come from the Hyprland event socket, not a poll. Nothing else is scheduled unless you actually asked for a file or live weather.
- Ranking cares when you last used something, not just how many times. An app from yesterday beats one you hammered two years ago. Typing the start of a name is no longer 200× heavier than a habit.
- Swapped letters count (`weahter` → weather). Missing letters still do (`wthr` → weather).
- A result can *show* something: live weather, a photo or video thumb, a document snippet. That is not the same as an icon, a title, and Enter.
- Misspellings are handled across apps, files, types, notes, and settings — not a fixed example list. `markdwon` still finds markdown, `readne` still finds `readme.md`, `firfox` still ranks Firefox if that app is installed.
- Selecting a result also fills the side preview: images, video frames, audio cover art and tags, the start of a document, a folder listing, or a play prompt. Enter or Space plays media in your default player; Enter opens other files in your editor.
- Thumbs reuse the Freedesktop cache (`~/.cache/thumbnails/large/`) when another app has already generated them.
- Type `markdown`, `pdf`, `images`, `*.rs`, or `type:md readme` to list matching files from home (and, when `plocate`/`locate` is available, the rest of the disk). Arrow keys and Page Up/Down scroll the full set.
- Open **Search Files** (`file`, Ctrl+F, or `flint --files`) for the dedicated long list. An empty query shows recent and frequently opened files.
- Calculator, unit conversion (`10 km to mi`, `32f`), hex colors (`#ff5a1f`), PATH binaries, and well-known folders (`Downloads`, `Documents`) appear as instant answers.
- Date and percent math too: `today + 7d`, `100 days from now`, `days until 2026-12-25`, `20% of 80`, `20% off 80`, `80 + 20%`. Type `calc` or `=` for recent answers (not dumped on an empty root query).
- Ctrl+K opens an action panel on the selected result (pin favorite, set alias, copy path, paste, open with). Ctrl+? still opens Ask AI. Ctrl+F files, Ctrl+N notes, Ctrl+, settings stay.
- Snippets expand `{clipboard}`, `{date}`, `{time}`, `{datetime}`, `{day}`, `{increment}`, and strip `{cursor}` on paste.
- Quicklinks (`link`) open URLs, folders, or files. `{argument}` / `{Query}` is the rest of the query after the keyword. `+gh https://github.com/search?q={argument}` saves one. Defaults: Downloads, Documents, GitHub search.
- Aliases: in the action panel, type a nickname then run **Set alias**. If the filter is empty, Flint puts `alias:` in the search box — finish the name and Enter. Aliases boost root ranking and match in the haystack.
- Pin favorites from the action panel; they float to the top of an empty root list.
- Result caps live in Settings (`max-results`) and `~/.config/flint/config.json` under `general.max_results` and `files.max_results`. Extra folders go in `files.search_roots`.

## Honest status

| Works | Not 1.0 |
| --- | --- |
| Daemon hide/toggle, apps, calc (math, dates, percents, history), clipboard pin/rename/edit, notes, snippets with placeholders, quicklinks, aliases, favorites, action panel, confetti, settings, store browse | Extensions: `List`, `Detail`, actions, navigation, toasts, storage — no `Form`, `Grid` layout, menu-bar, or extension OAuth yet |
| Installed Vicinae / Raycast extensions run in a Node host (real React + `@vicinae/api`) | Extensions are **off by default** and run unsandboxed as your user when you opt in |
| Ask AI against local Ollama / LM Studio / llama.cpp and configured cloud APIs | Consumer ChatGPT and Claude plans do not include API usage |
| `pw-record` + voxtype dictation into the search box | Third-party script-commands are **off by default** and run as `sh` / `python3` / `node` with no signature when you opt in |
| PKCE OAuth + loopback `127.0.0.1` + refresh tokens | MCP is a prompt primer; the model cannot run tools. MCP spawn is **off by default** |

## Ask AI

Local models first. Flint scans Ollama (`http://127.0.0.1:11434`), LM Studio (`:1234`), and llama.cpp (`:8080`) and lists whatever is already running.

Flint supports Google Gemini API OAuth and standards-compatible custom OAuth
providers. Configure a desktop client ID, authorize URL, token URL, scopes, API
endpoint, and model in Settings. The system browser handles sign-in; Flint uses
PKCE S256 and an exact `127.0.0.1` callback, refreshes expiring access tokens,
and refuses to send an OAuth token to a different API origin.

OAuth authorizes API access only when the provider supports it. A consumer
subscription is not automatically an API entitlement: ChatGPT plans and Claude
plans are billed separately from their developer APIs. OpenAI and Anthropic are
therefore configured with provider API keys, not unsupported consumer OAuth.
Google OAuth also requires the Google Cloud quota project ID in Settings.

Credentials are stored in the Linux desktop Secret Service when available. On
desktops without a usable keyring, Flint falls back to a mode-`600` credential
file and reports that choice after saving. Local models need no credential.

## Dictation

Enter starts an in-app recording (`pw-record`). Enter again transcribes with voxtype and **fills the search box**. Esc cancels. The WAV is deleted after transcribe or cancel.

## Store

Raycast’s App Store is proprietary and is not connected. Flint’s store:

- Vicinae extensions from [`vicinaehq/extensions`](https://github.com/vicinaehq/extensions) — installed commands show up in root search and run in Flint when you opt in
- MCP servers (filesystem, git, fetch, memory) — listed for the model as a primer, only if you enable MCP in Settings
- Sync of the public [`raycast/script-commands`](https://github.com/raycast/script-commands) repo — running them requires the Settings toggle

## Extensions

Flint runs Vicinae and Raycast-style extensions as one Node process per command. The host (`share/runtime/flint-host.js`) loads real `react` 19 and `@vicinae/api`, drives them with a small `react-reconciler`, and streams the host-element tree to Flint as JSON. Flint paints that in its normal result list.

- Install from the Store, then enable **Run installed extensions** in Settings. Commands appear in root search under the extension's name.
- First launch runs `npm install` once into `~/.local/share/flint/runtime/` (pinned `react`, `react-reconciler`, `@vicinae/api`, `esbuild`) and bundles the command with `esbuild` when sources change. `node` and `npm` must be on `PATH`.
- Enter runs the item's first action, Shift+Enter the second. Esc pops a pushed view, then leaves the extension. Hiding the window keeps a running view-command alive.
- Supported: `List` (sections, accessories, keywords, icons), `Detail`, `ActionPanel`, `useNavigation`, `showToast`, `showHUD`, `Clipboard`, `LocalStorage`, `Cache`, `getPreferenceValues` (manifest defaults), `open`, `runInTerminal`, `closeMainWindow`, `popToRoot`, no-view commands. `@raycast/api` imports are aliased to `@vicinae/api`.
- Not yet: `Form`, `Grid` layout (grids render as lists), `MenuBarExtra`, search-bar dropdowns, extension OAuth, `getSelectedText`, command arguments, editing preferences in Settings, `confirmAlert` (cancels), file-search RPC.
- Extension `console.log` output goes to Flint's stderr, tagged with the extension name. `LocalStorage` lives in `~/.local/share/flint/extensions/<name>/`.

## Privacy and safety

- Config, credential fallbacks, snippets, notes, and clipboard files are mode `600` under directories mode `700`
- Clipboard history skips common secret patterns (API keys, tokens, PEM blocks)
- Attaching clipboard to Ask AI redacts the same patterns
- HTTPS AI / OAuth calls keep bearer tokens out of `ps` (curl `-K` config file, then deleted)
- Unsigned script-commands, MCP process spawn, and installed extensions are off until you turn them on; extensions run as Node with your user's privileges
- OAuth callback accepts only the expected HTTP/1.1 `GET /callback`, exact loopback Host/port, and constant-time state match
- OAuth tokens are bound to the selected provider and API origin and are refreshed without putting secrets on process arguments

See [SECURITY.md](SECURITY.md) for controls and vulnerability reporting, and
[PRIVACY.md](PRIVACY.md) for local/external data flow.

## Paths

- Config: `~/.config/flint/config.json`
- Auth metadata or credential fallback: `~/.config/flint/auth.json`
- API-key fallback (when no Secret Service is available): `~/.config/flint/api-keys.json`
- Snippets: `~/.config/flint/snippets.json`
- Aliases: `~/.config/flint/aliases.json`
- Quicklinks: `~/.config/flint/quicklinks.json`
- Notes: `~/.local/share/flint/notes.json`
- Clipboard: `~/.local/share/flint/clipboard.json`
- Favorites: `~/.local/share/flint/favorites.json`
- Calc history: `~/.local/share/flint/calc-history.json`
- Extension runtime: `~/.local/share/flint/runtime/` · installed extensions: `~/.local/share/flint/store/vicinae/<name>/` · their storage: `~/.local/share/flint/extensions/<name>/`

Existing Rayblast files are copied over on first launch. Stale Rayblast defaults (OpenAI provider + Ollama endpoint, “You are Rayblast”, `voice.engine: voxtype`) are rewritten to Flint defaults.

## Build

```sh
cargo test
cargo build --release
```

License: MIT. Issues and PRs: [github.com/aco-linux/flint](https://github.com/aco-linux/flint).
