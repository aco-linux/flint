# Changelog

## Unreleased — launcher map

- Stop the open animation that tiles Flint large then shrinks it: Hyprland now floats, sizes, and centers on the first frame (`no_anim`), and Flint no longer re-dispatches float/resize after map

- Opt-in Node host for installed Vicinae / Raycast-style extensions (`general.allow_extensions`, off by default). Each command is one Node process with real `react` 19 and `@vicinae/api`; List/Detail rows render in Flint's result list
- Enter runs the first action, Shift+Enter the second; Esc pops a pushed view then leaves. Clipboard, open, terminal, LocalStorage, toasts, and no-view commands work. Confirm dialogs cancel until Flint has a real prompt
- Runtime (`~/.local/share/flint/runtime/`) is installed with pinned `npm` packages on first launch; command sources are bundled with `esbuild`. Config is sent on stdin, not argv
- Not yet: `Form`, Grid layout, menu-bar, extension OAuth, preference editing, selected-text, or file-search RPC

## 0.4.0 — floating window and media thumbs

- Opens as a floating, resizable Hyprland window (`dev.flint.launcher`) instead of a large tile
- Image and video thumbs share one producer: Freedesktop `thumbnails/large/` cache first, then pixbuf or one `ffmpeg` frame
- Generated thumbs are written back as `file://` MD5 PNGs so other apps can reuse them
- Video extract is cancellable (same process-group SIGKILL as file search) so arrowing does not leave `ffmpeg` running
- Audio preview shows duration, bitrate, and tags, plus embedded cover art when present
- Spacebar on audio/video plays in the default player (same action as Enter) and does not insert a space

## 0.3.0 — search, ranking, and dictation

- Starting dictation no longer aborts: the GTK callback dropped its `RefCell` borrow before updating status
- File search understands type words (`markdown`, `pdf`, `images`), extensions (`*.md`), and filters (`type:md readme`)
- Search Files mode (`file`, Ctrl+F, `--files`) returns a long scrollable list instead of 12 rows
- Root search mixes apps with many more file hits; `plocate`/`locate` covers folders outside `$HOME`
- Instant answers: unit conversion, hex colors, PATH commands, and well-known folders
- Page Up/Down jumps the list; the status line shows how many files matched
- Intent engine: `we` is weather; live conditions come from wttr.in
- Typo tolerance is general: Damerau–Levenshtein against app titles, file type words, filenames, notes, and settings — not a hardcoded example list
- Swapped letters match (`weahter` → weather); the old matcher only rewarded missing letters
- Fast path no longer shells out to `fd` or `hyprctl` per keystroke
- Hyprland window list is pushed over `.socket2.sock`; keystrokes never poll `hyprctl`
- One cancellable file worker; a new query SIGKILLs the in-flight process group instead of stacking threads or forking `kill`
- File previews read only the first bytes; images decode off the UI thread and cache
- Usage ranking includes recency; a prefix bonus is no longer ~200× one use
- Results have a live slot: weather, image thumbs, and document snippets render in the row
- In-launcher side preview for images, text, folders, and playable media; Enter plays or opens to edit

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
