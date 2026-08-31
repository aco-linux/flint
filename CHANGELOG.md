# Changelog

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
