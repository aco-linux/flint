# Flint

A native GTK4 command launcher for Linux (Wayland / Hyprland). Super+Space.

This is **public 0.1**, not “Raycast for Linux.” Core loops work. There is no JS extension host. The store clones Vicinae folders and Raycast script-commands; it does not run Raycast extensions inside Flint.

![Flint](share/flint.png)

Flint stays resident: the first launch keeps a daemon so clipboard history, notes, dictation, and Ask AI stay warm.

## Install

Needs a Rust toolchain, GTK4, and [gtk4-layer-shell](https://github.com/wmww/gtk4-layer-shell).

```sh
git clone https://github.com/aco-linux/flint.git
cd flint
make install
```

That puts `flint` in `~/.local/bin` and a desktop entry under `~/.local/share/applications`. Then bind it:

```
exec-once = flint --daemon
bind = SUPER, SPACE, exec, flint
bind = SUPER SHIFT, R, exec, flint --windows
```

On Omarchy, Super+Space is the system menu until you unbind it. Bindings live in `~/.config/hypr/bindings.lua`. A full snippet is in [`share/hyprland.conf`](share/hyprland.conf).

## Modes

| Prefix | Mode | Also |
| --- | --- | --- |
| _(empty)_ | Apps, files, calc, extensions | Super+Space |
| `win` | Window switcher | `--windows` |
| `clip` | Clipboard history | `--clipboard` |
| `;` / `snip` | Snippets | `--snippets` |
| `note` | Notes | `--notes` |
| `?` / `ask` | Ask AI | `--ask` |
| `voice` | Dictation | `--voice` |
| `set` | Settings | `--settings` |
| `store` | Store | `--store` |

Type `+keyword` in snippets to save the clipboard. Type `+title` in notes to create one. Prefix `>` to run a command, `$` to run it in a terminal.

## Honest status

| Works | Not 1.0 |
| --- | --- |
| Daemon hide/toggle, apps, calc, clipboard, notes, snippets, settings, store browse | No JS extension host |
| Ask AI against local Ollama / LM Studio / llama.cpp | Cloud OAuth needs *your* client ID — ChatGPT Plus / Gemini Advanced do not sign in magically |
| `pw-record` + voxtype dictation into the search box | Third-party script-commands are **off by default** and run as `sh` / `python3` / `node` with no signature when you opt in |
| PKCE OAuth + loopback `127.0.0.1` | MCP is a prompt primer; the model cannot run tools. MCP spawn is **off by default** |

## Ask AI

Local models first. Flint scans Ollama (`http://127.0.0.1:11434`), LM Studio (`:1234`), and llama.cpp (`:8080`) and lists whatever is already running.

Cloud providers use **OAuth in the browser**, not a pasted API key, whenever you can. Add your own OAuth client ID in Settings, then **Sign in with OpenAI**, **Google**, or a custom authorize URL. Tokens live in `~/.config/flint/auth.json` (mode 600). An API key is only a fallback. Custom OAuth URLs must be `https://`.

## Dictation

Enter starts an in-app recording (`pw-record`). Enter again transcribes with voxtype and **fills the search box**. Esc cancels. The WAV is deleted after transcribe or cancel.

## Store

Raycast’s App Store is proprietary and is not connected. Flint’s store:

- Vicinae extensions from [`vicinaehq/extensions`](https://github.com/vicinaehq/extensions) (cloned onto disk; they do not run inside Flint)
- MCP servers (filesystem, git, fetch, memory) — listed for the model as a primer, only if you enable MCP in Settings
- Sync of the public [`raycast/script-commands`](https://github.com/raycast/script-commands) repo — running them requires the Settings toggle

## Privacy and safety

- Config, auth, snippets, notes, and clipboard files are mode `600` under directories mode `700`
- Clipboard history skips common secret patterns (API keys, tokens, PEM blocks)
- Attaching clipboard to Ask AI redacts the same patterns
- HTTPS AI / OAuth calls keep bearer tokens out of `ps` (curl `-K` config file, then deleted)
- Unsigned script-commands and MCP process spawn are off until you turn them on
- OAuth callback only accepts `GET /callback` on `127.0.0.1`

See [SECURITY.md](SECURITY.md) for how to report issues.

## Paths

- Config: `~/.config/flint/config.json`
- Auth: `~/.config/flint/auth.json`
- Snippets: `~/.config/flint/snippets.json`
- Notes: `~/.local/share/flint/notes.json`
- Clipboard: `~/.local/share/flint/clipboard.json`

Existing Rayblast files are copied over on first launch. Stale Rayblast defaults (OpenAI provider + Ollama endpoint, “You are Rayblast”, `voice.engine: voxtype`) are rewritten to Flint defaults.

## Build

```sh
cargo test
cargo build --release
```

License: MIT. Issues and PRs: [github.com/aco-linux/flint](https://github.com/aco-linux/flint).
