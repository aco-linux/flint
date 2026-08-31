# Changelog

## Unreleased

- File search understands type words (`markdown`, `pdf`, `images`), extensions (`*.md`), and filters (`type:md readme`)
- Search Files mode (`file`, Ctrl+F, `--files`) returns a long scrollable list instead of 12 rows
- Root search mixes apps with many more file hits; `plocate`/`locate` covers folders outside `$HOME`
- Instant answers: unit conversion, hex colors, PATH commands, and well-known folders
- Page Up/Down jumps the list; the status line shows how many files matched
- Intent engine: `we` is weather; live conditions come from wttr.in
- Typo tolerance is general: Damerau–Levenshtein against app titles, file type words, filenames, notes, and settings — not a hardcoded example list
- Results update as you type; file search and weather fill in without freezing the UI
- In-launcher previews for images, text, folders, and playable media; Enter plays or opens to edit

## 0.2.0 — release hardening

- Replaced the fullscreen layer-shell surface with a normal movable, resizable,
  minimizable, and maximizable GTK window
- Added standards-compatible custom OAuth plus a documented Google Gemini API
  OAuth preset; removed the unsupported consumer OpenAI OAuth claim
- Added PKCE S256, exact loopback callback parsing, constant-time state checks,
  structured token responses, refresh tokens, and provider/API-origin binding
- Moved OAuth tokens and provider API keys into the desktop keyring when
  available, with a clearly reported private-file fallback
- Added AppStream metadata, privacy documentation, dependency auditing, lint,
  format, test, release-build, and desktop-metadata CI gates
- Fixed the desktop entry so launching Flint opens the application window

## 0.1.0 — public MVP

- Native GTK4 launcher for Wayland / Hyprland (Alt+Space; Super+Space stays Omarchy)
- Apps, calc, files, window switcher, clipboard, notes, snippets, Ask AI, dictation, settings, store
- Private files mode 600 / dirs 700; clipboard secret skip; curl `-K` for HTTPS tokens
- Unsigned script-commands and MCP spawn **off by default**
- OAuth PKCE on loopback `127.0.0.1`; user-supplied client ID only
- Honest about what it is not: no JS extension host, not Raycast
