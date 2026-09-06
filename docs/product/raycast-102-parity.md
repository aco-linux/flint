# Flint capability: Raycast 2.0 "102 things" parity

Source: [102 Things You Can Do With Raycast 2.0](https://youtu.be/G7_7F_FBqQE) (Raycast, 2026-09-04).
Companion numbered list: the 101-things description, updated in-place by the 2.0 video.

This is a capability contract, not a promise that Flint clones Raycast's proprietary store, iPhone app, or cloud account.

## CAPABILITY

A Hyprland / Wayland user can complete the same *jobs* shown in that video from Flint's command bar: launch, find, clip, type, calculate, window, capture, chat, extend, and a handful of playful extras. Linux-native tools stand in for Mac-only surfaces. Flint stays local-first; nothing requires a Raycast account.

## CONSTRAINTS

- Flint is a Linux GTK4 launcher for Wayland / Hyprland. It does not ship a Mac, Windows, or iOS client.
- Local-first: notes, snippets, clipboard, and credentials stay on disk / Secret Service. Cloud sync is optional and later, not a 1.0 gate.
- Raycast's App Store, Organization, iPhone sync, and `ray.so` SaaS are proprietary. Flint uses Vicinae extensions, Raycast *script-commands* (opt-in), and MCP primers.
- Installed extensions, unsigned scripts, and MCP spawn stay **off by default**.
- Do not vendor CleanShot, Raycast Focus, Raycast Notes cloud, or Raycast AI billing.
- Prefer compositor / desktop tools already on Omarchy (Hyprland, grim, slurp, wf-recorder, playerctl, pacman, Flatpak) over new daemons. Commands for optional binaries (tesseract, zbarimg, hyprpicker, satty, wf-recorder, grim) appear only if found.
- Each capability is a user-visible job. Extensions may satisfy a job; core must still expose a command or store install path.

## Performance invariants

These are testable. Remaining waves must not violate them.

- Hidden daemon: 0% CPU at idle. No periodic timers. Clipboard ingest is the GDK `changed` signal only — never a 1s `wl-paste` poll.
- Root-search keystroke: results from in-memory catalogs within one frame. Anything slower (ripgrep, OCR, extension host, tesseract) is behind an explicit prefix and cancelled on the next keystroke.
- No always-on indexers. No recoll. Content search is `content:` + `rg --max-count 1` over `$HOME` and `search_roots`, never `/`.
- Extension host is lazy-spawned and must not change idle RSS when `allow_extensions` is off.
- Confetti: fixed ≤1.5s, frame-capped cairo overlay, not a compositor-wide layer unless gtk4-layer-shell is already in the tree.
- User data (clips, notes, snippets, aliases, favorites, calc history, usage, quicklinks, layouts) lives in one SQLite file with incremental writes. Config and credentials stay JSON / Secret Service.
- Snippets and quicklinks share one placeholder engine (`{clipboard}`, `{date}`, `{time}`, `{datetime}`, `{day}`, `{increment}`, `{cursor}`, `{argument}` / `{Query}`, `{selection}`).

Status key:

| Status | Meaning |
| --- | --- |
| **ships** | Present in Flint today at useful quality |
| **partial** | Exists, missing depth the video shows |
| **core** | Should be built into Flint |
| **ext** | Store / extension / script-command, not a new core module |
| **desktop** | Compositor, keyd/kanata, or Omarchy helper — Flint exposes a command, does not own the mechanism |
| **out** | Mac / iPhone / Raycast-cloud only; Linux equivalent is a different product |

## The 102 jobs

Numbering follows the 2.0 narration. Closely related beats in the video are split when they are distinct jobs.

| # | Job | Flint now | Lane |
| --- | --- | --- | --- |
| 1 | Launch applications | Desktop files in root search | ships |
| 2 | Search files by name | `file` / root mix, fd + plocate | ships |
| 3 | Search files by content | Not indexed; preview reads first bytes only | core |
| 4 | Navigate the filesystem from the bar | Path-like queries, well-known folders; no Finder-style drill-down | partial |
| 5 | Clipboard history | Resident daemon, 80 text entries, secret skip | ships |
| 6 | Organize clipboard | Pins (no tags/folders) | ships |
| 7 | Rename a clipboard entry | Action panel rename | ships |
| 8 | Edit clipboard content | Action panel edit; secrets still rejected | ships |
| 9 | Search GIFs | Missing | ext |
| 10 | Search emojis | Shells `omarchy-menu-emoji` if present | partial |
| 11 | Search emojis with AI | Missing | ext |
| 12 | Search Slack emojis | Missing | ext |
| 13 | Dictate from anywhere | In-launcher only (`pw-record` + voxtype → search box) | core |
| 14 | Dictate in any language | `voice.language` setting, not a first-class picker | partial |
| 15 | Dictate with styles | Missing | core |
| 16 | Remember everything you said | No dictation history | core |
| 17 | Arrange windows in ~58 presets | Halves, quarters, maximize, center, almost-maximize, next/prev display | ships |
| 18 | Custom window management commands | Named layouts in `layouts.json`; `layout +name` saves | ships |
| 19 | Caps Lock as Hyper key, Caps Lock still works | Not a launcher feature | desktop |
| 20 | Nicknames for apps | Aliases via Ctrl+K / `alias:` | ships |
| 21 | Nicknames for commands | Same alias map | ships |
| 22 | Reusable text snippets | `;` / `snip`, `+keyword` save | ships |
| 23 | Dynamic placeholders in snippets | `{clipboard}`, `{date}`, `{time}`, `{datetime}`, `{day}`, `{cursor}` | ships |
| 24 | Auto-increment snippet placeholders | `{increment}` per snippet | ships |
| 25 | Emoji keywords | Missing | core |
| 26 | Ask AI throwaway questions | `?` / Ask mode, local + OAuth/API | ships |
| 27 | Follow-up AI turns | One-shot; no chat thread | core |
| 28 | Take notes from anywhere | `note` / `+title` | partial |
| 29 | Capture selected text into a note | No `getSelectedText` | core |
| 30 | Simple math | `evalexpr` instant answer | ships |
| 31 | Complicated math | Same evaluator; no CAS | partial |
| 32 | Convert units | Length, mass, data, temperature | partial |
| 33 | Date calculations | `today + 7d`, `days until`, `days since` | ships |
| 34 | Percentage / discount calculations | `% of` / `% off` / `+ %` | ships |
| 35 | Calculation history | `calc` / `=` private JSON | ships |
| 36 | Hotkeys to open favorite apps | Hyprland binds Flint; no per-app hotkeys | core |
| 37 | Single-key command aliases | Missing | core |
| 38 | Double-tap to a folder | Missing | core |
| 39 | Search screenshots by content | File search is name/type, not OCR | core |
| 40 | Stay on top of schedule | Missing | ext |
| 41 | Join online meetings | Missing | ext |
| 42 | Join meetings automatically | Missing | ext |
| 43 | Check appearance before joining | Missing (webcam preview) | ext |
| 44 | Take a selfie | Missing | ext |
| 45 | Quicklinks — websites | User links + `{argument}` | ships |
| 46 | Quicklinks — files | Path targets | ships |
| 47 | Quicklinks — folders | `~/Downloads` etc. | ships |
| 48 | Quicklinks — app deep links | http/https/file URIs only | partial |
| 49 | Screen recording | `wf-recorder` command; second invoke stops | desktop |
| 50 | Screenshot | grim, else Omarchy capture helpers | ships |
| 51 | Annotate a screenshot | satty or swappy after a region capture | desktop |
| 52 | Chat with current AI models | Ollama / LM Studio / llama.cpp / cloud keys | partial |
| 53 | Make models *do* things (tools) | MCP is a prompt primer; model cannot run tools | core |
| 54 | Quit an application | Action panel + closewindow; force-quit is SIGKILL | ships |
| 55 | Auto-quit applications | Missing | core |
| 56 | Quit all applications at once | Confirm result; denylist in `quit-keep.json` | ships |
| 57 | Fix grammar and spelling | Missing | core |
| 58 | Inline Quick Fix | Missing | core |
| 59 | Uninstall applications | Flatpak / pacman when the desktop file maps; confirm first | ships |
| 60 | Start a focus session | Missing | core |
| 61 | Unfocus / break | Missing | core |
| 62 | Download thousands of extensions | Vicinae store + script-commands; not Raycast's catalog | partial |
| 63 | Build an extension with AI | Missing | out |
| 64 | Build an extension by hand | Vicinae API host (`List`/`Detail`); no `Form`/`Grid`/OAuth | partial |
| 65 | Time at a destination | Local time item only | core |
| 66 | Time difference between two cities | Missing | core |
| 67 | Switch display resolution | `hyprctl` modes plus 720p/1080p/1440p/4K | desktop |
| 68 | Convert images | Missing | ext |
| 69 | Ask AI to convert images | No image-out tools | ext |
| 70 | Translate text | Missing | core |
| 71 | Translate specific language pairs | Missing | core |
| 72 | Quick word lookup | Missing | ext |
| 73 | Copy file path | Action panel | ships |
| 74 | Chat with a Hermes agent | Default local model is `qwen3.5:9b-hermes`; not an agent runtime | partial |
| 75 | Open Claw / OpenClaw | Missing | ext |
| 76 | Search fonts | Missing | ext |
| 77 | Pick a color | Hex parse only (`#ff5a1f` → RGB) | partial |
| 78 | Convert color formats | Hex → RGB copy; no HSL/OKLCH/picker | core |
| 79 | Sync two Macs | n/a | out |
| 80 | Sync Mac and PC | n/a | out |
| 81 | Sync computers and iPhone | n/a | out |
| 82 | Tags on snippets | Missing | core |
| 83 | Tags on quicklinks | Missing (no quicklinks) | core |
| 84 | Menu-bar extras | Waybar is the Linux surface | desktop |
| 85 | Browse a Notion workspace | Missing | ext |
| 86 | Modify system settings | Flint settings + a few Omarchy launchers | partial |
| 87 | Practice typing | Missing | ext |
| 88 | Search commands in the focused app | Missing (no GTK/AT-SPI menu scrape yet) | core |
| 89 | Pin favorites to the top | Action panel pin; empty root lists them first | ships |
| 90 | Custom AI agents | Missing | core |
| 91 | Launch an agent with a hotkey | Missing | core |
| 92 | AI knows who you are | System prompt only | core |
| 93 | AI remembers what you do | Missing | core |
| 94 | Read text from images (OCR) | Missing | core |
| 95 | Decipher QR codes | Missing | core |
| 96 | Teach AI skills | Missing | core |
| 97 | Extend AI capabilities | MCP primer; no tool runner | core |
| 98 | Autonomous “just do it” | Missing | core |
| 99 | Share the screen with AI | Missing | core |
| 100 | Share a region with AI | Missing | core |
| 101 | *(video beat: AI skills doing work unattended — counted in 96–98)* | — | — |
| 102 | Throw confetti | In-window overlay | ships |

Count after Wave 1: **~38 ships**, **~12 partial**, **~36 core**, **~12 ext**, **~6 desktop**, **~5 out**. The video's "102" is a narration, not a spec; 101 is the last numbered job before confetti in the 1.0 list, confetti is 102.

## IMPLEMENTATION CONTRACT

### Actors

- **User** at a Hyprland keyboard: Alt+Space, optional Super+Shift mode binds.
- **Resident daemon**: clipboard ingest, hide/show, dictation, Ask AI.
- **Node extension host** (opt-in): Vicinae / Raycast-style commands.
- **Desktop**: Hyprland, grim/slurp, wf-recorder, keyd, playerctl, pacman/flatpak.

### Surfaces

- Root search (empty prefix)
- Mode prefixes already shipped: `file`, `win`, `clip`, `;`, `note`, `?`, `voice`, `set`, `store`
- New modes likely: `link` (quicklinks), `calc` history, `focus`, action panel (Ctrl+K)
- Global hotkeys owned by Hyprland; Flint writes suggested binds, does not steal the compositor

### Invariants

- Content search, OCR, and screenshot index never leave the machine unless the user sends them to Ask AI.
- Dictation WAV is deleted after transcribe/cancel (already true).
- Quit / uninstall always confirm when the target is not a user-owned window.
- Quicklinks and snippets live in the user SQLite file (`flint.db`, mode 600), same as clips and notes. Config and credentials stay JSON / Secret Service.
- Hyper key, display mode, and screen record are commands that *invoke* desktop tools; Flint does not become an input daemon.

### Non-goals

- Byte-for-byte Raycast UI, Pro billing, Organization, or iPhone.
- Replacing Hyprland window rules with a second compositor.
- A Flint-hosted cloud sync of snippets/quicklinks in this lane.
- Shipping a JS extension builder ("build with Raycast AI").
- Apple Music / Apple Notes / Homebrew as named features.

## Delivery waves

Each wave is independently shippable. Do not start wave N+1 until wave N is in CHANGELOG and tests.

### Wave 1 — Command bar fundamentals

Jobs: 6–8, 20–21, 23–25, 33–38, 45–48, 73, 89, 102.

- Clipboard: pin, rename, edit, paste-plain.
- User aliases (nicknames) on any item.
- Per-command / per-app hotkey suggestions written to `share/hyprland.conf` snippets; Flint also honors in-app single-key aliases while open.
- Snippet placeholders: `{clipboard}`, `{date}`, `{time}`, `{cursor}`, `{increment}`.
- Date and percent parsers next to the existing calculator.
- Calculation history (same private JSON pattern as clips).
- Quicklinks: URL / path / folder / custom URI, with `{argument}` placeholders.
- Action panel: Copy Path, Open With, Pin, Paste, Show in files.
- Confetti on a command (and optionally on first-run).

### Wave 2 — Windows and system

Jobs: 17–18, 49–51, 54–56, 59, 67, 86.

- Hyprland layouts: left/right halves, quarters, maximize, center, next/prev display; user-named layouts.
- Quit / kill focused or named app; quit all except a denylist.
- Uninstall via pacman / Flatpak when the desktop file maps to a package.
- Screenshot / region / record / annotate as Flint commands wrapping grim, slurp, wf-recorder, satty.
- Display resolution via `hyprctl` / `wlr-randr`.

### Wave 2.5 — Lightness (before more stores)

- One SQLite file (`~/.local/share/flint/flint.db`, mode 600) for clips, notes, snippets, aliases, favorites, calc history, usage, quicklinks, layouts, quit-keep. Import existing JSON once.
- Shared placeholder module used by snippets and quicklinks.
- Drop the 1s clipboard poll; GDK `changed` only.
- Size-based clipboard cap (not a raw 80-row cap). Keep secrets out.

### Wave 3 — In-bar content tools

Jobs: 3, 10, 25, 39 (on demand), 65–66, 68, 76–78, 94–95.

- Content search: explicit `content:` prefix, `rg --max-count 1`, cancel on next keystroke. No indexer.
- Built-in emoji table (~200); GIF/Slack remain extensions.
- Timezones as a static city table (no extra crate unless evalexpr is clearly insufficient).
- Color picker only if `hyprpicker` / `wl-color-picker` exists; hex/rgb/hsl convert in-process.
- Translate and dictionary are **Ask-AI prompt templates**, not a network module. Currency rates are opt-in and cached; skip until asked.
- OCR (`tesseract`) and QR (`zbarimg`) only if on PATH. No sidecar index.

### Wave 4 — Dictation, notes, focus

Jobs: 13–16, 28–29, 60–61, 84.

- Global dictation via `wtype` (Hyprland virtual-keyboard). Do not use ydotool.
- Job 29 is **partial**: `wl-paste -p` primary selection, not a Wayland selected-text API.
- Dictation history in SQLite.
- Focus timer writes `~/.local/share/flint/status.json` for a Waybar custom module.

### Wave 5 — AI that can act

Jobs: 27, 52–53, 57–58, 70–71, 74, 90–93, 96–100.

- Threaded Ask AI in SQLite; grammar/Quick Fix/translate are prompt templates on `{selection}` / primary paste.
- Attachments (file, clipboard, region). MCP tools still opt-in.
- Memory and skills local. Screen share is an attachment, never a spy.

### Wave 6 — Store and desktop glue

Jobs: 19, 36–38, 40–44, 62–64, 75, 85, 87.

- Finish extension host: Form, Grid, confirm, preferences. Selected-text uses primary paste.
- Hotkeys: write/source `~/.config/hypr/flint-binds.conf` (Hyprland hot-reloads). No helper daemon.
- Calendar / meetings / Notion / OpenClaw / typing as store extensions.
- Hyper key: document keyd/kanata. Job 88 (app menu / AT-SPI) is **later / ext** — never poll the a11y bus.

## OPEN QUESTIONS

1. **Hotkeys:** **Decided.** Sourced `~/.config/hypr/flint-binds.conf`. No daemon.
2. **Content index:** **Decided.** Explicit `content:` + bounded `rg`. No recoll.
3. **Global dictation:** **Decided.** `wtype`. Job 29 = primary selection.
4. **Sync:** Stay local-forever for now; later optional Syncthing of snippets + quicklinks (never clipboard secrets).
5. **Confetti:** **Decided.** In-window cairo overlay, ≤1.5s, frame-capped.
6. **Calculator crate:** Keep evalexpr + existing date/percent for now. Revisit numbat/fend only if those parsers fail users. Currency is opt-in later.

## HANDOFF

Ready for implementation **one wave at a time**. Wave 1 is the first PR stack: it is all core launcher work, needs no new network services, and is what makes Flint feel like the video before AI/cloud extras.

Do not implement Waves 2–6 in the same PR as Wave 1.
