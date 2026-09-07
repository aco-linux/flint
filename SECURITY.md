# Security

Flint is a local desktop launcher. A bug here can run commands, read the clipboard, or leak tokens. Treat it that way.

## Report a vulnerability

Open a **private** GitHub security advisory on [aco-linux/flint](https://github.com/aco-linux/flint/security/advisories/new), or email the maintainer listed on the GitHub profile. Do not file a public issue for an exploitable bug.

Please include:

- Flint version (`flint` binary date / git commit)
- Distro and compositor (Hyprland / other)
- Steps to reproduce
- What an attacker would gain

## What 0.2 does

| Control | Behavior |
| --- | --- |
| Credential storage | Linux Secret Service first; explicit mode-`600` fallback when unavailable |
| File modes | Config/auth/API-key fallback and `flint.db` are `600`, dirs `700` |
| Clipboard | Heuristic skip for common secrets; not a detector |
| Ask AI | HTTPS via curl `-K` so tokens are not on `ps`; HTTP refuses `Authorization` / `x-api-key` |
| OAuth | PKCE S256, 256-bit state, exact loopback Host/port, bounded HTTP parser, HTTPS authorize/token URLs, structured token parsing, refresh support |
| Token use | Provider match and API-origin binding are checked before a bearer token leaves the device |
| Scripts | Off by default. When on, only files under Flint’s store dir, `sh`/`python3`/`node` |
| Extensions | Off by default. When on, each command is a Node process with full Node APIs; runtime packages are version-pinned in `src/extension.rs` |
| MCP | Off by default. Spawn allow-list is `npx` only. Tools are listed, not executed |
| Git | Clone/pull only into `~/.local/share/flint/store`, `protocol.file.allow=never` |
| URIs | `http`, `https`, `file` only |
| Voice | Kills the stored `pw-record` pid, deletes `voice.wav` |

## Trust boundaries and limitations

- Sandbox script-commands or extensions (once you opt in, they are your user)
- Filter every secret from clipboard history
- Turn a consumer ChatGPT, Claude, or other chat subscription into API access; only provider-supported API OAuth works
- Ship a shared OAuth client ID; a production distributor must register and verify its own provider clients where required
- Guarantee that every Linux session has an unlocked Secret Service; the UI reports when mode-`600` fallback storage is used
- Run every Vicinae / Raycast extension API (Grid layout, OAuth, preference editing, AT-SPI)
- Verify git tags or script signatures
- Protect you from a malicious `npx` package if you enable MCP

If you enable unsigned scripts, MCP, or extensions, you are running third-party code as yourself. Extensions run in a plain Node process; the runtime dependencies are pinned by version, not by hash.

Custom OAuth configuration is an expert feature. Flint validates transport,
callback, state, provider, and API origin, but the user still chooses which
authorization server, token server, scopes, and API endpoint to trust.
