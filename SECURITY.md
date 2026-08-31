# Security

Flint is a local desktop launcher. A bug here can run commands, read the clipboard, or leak tokens. Treat it that way.

## Report a vulnerability

Open a **private** GitHub security advisory on [aco-linux/flint](https://github.com/aco-linux/flint/security/advisories/new), or email the maintainer listed on the GitHub profile. Do not file a public issue for an exploitable bug.

Please include:

- Flint version (`flint` binary date / git commit)
- Distro and compositor (Hyprland / other)
- Steps to reproduce
- What an attacker would gain

## What 0.1 actually does

| Control | Behavior |
| --- | --- |
| File modes | Config/auth/notes/clipboard/snippets are `600`, dirs `700` |
| Clipboard | Heuristic skip for common secrets; not a detector |
| Ask AI | HTTPS via curl `-K` so tokens are not on `ps`; HTTP refuses `Authorization` / `x-api-key` |
| OAuth | PKCE, loopback `127.0.0.1`, `https://` authorize/token only, GET `/callback` + local Host |
| Scripts | Off by default. When on, only files under Flint’s store dir, `sh`/`python3`/`node` |
| MCP | Off by default. Spawn allow-list is `npx` only. Tools are listed, not executed |
| Git | Clone/pull only into `~/.local/share/flint/store`, `protocol.file.allow=never` |
| URIs | `http`, `https`, `file` only |
| Voice | Kills the stored `pw-record` pid, deletes `voice.wav` |

## What 0.1 does not do

- Sandbox script-commands (once you opt in, they are your user)
- Filter every secret from clipboard history
- Ship a shared OAuth client ID
- Run Vicinae / Raycast JS extensions
- Verify git tags or script signatures
- Protect you from a malicious `npx` package if you enable MCP

If you enable unsigned scripts or MCP, you are running third-party code as yourself.
