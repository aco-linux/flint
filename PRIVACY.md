# Privacy

Flint is a local-first desktop application. It has no Flint account, analytics,
advertising SDK, or maintainer-operated cloud service.

## Data kept on this computer

Flint stores settings, notes, snippets, clipboard history, usage ranking, and
extension data under the standard user config/data directories. Private data
files are written with mode `600` inside directories with mode `700`.

OAuth and API credentials are stored in the Linux desktop Secret Service when
`secret-tool` and a keyring are available. If the desktop has no usable Secret
Service, Flint falls back to a local mode-`600` credential file and says so in
the UI. OAuth access tokens are bound to the provider and API origin that were
active at sign-in.

## Data sent off the computer

Flint sends data only when a feature requires it:

- Ask AI sends the prompt and system prompt to the selected AI endpoint.
- Clipboard text is included only when “Attach clipboard to Ask AI” is enabled.
- OAuth opens the provider's authorization page and exchanges the returned code
  with the configured token endpoint.
- Store sync contacts the public repositories listed in the app.
- MCP, unsigned script commands, and installed extensions can start third-party
  programs only after the user enables the corresponding setting. Enabling
  extensions also lets Flint run `npm install` for the pinned runtime packages
  and for each extension's own dependencies.

Those third parties apply their own privacy and retention terms. Flint does not
proxy those requests and the maintainer does not receive their contents.

## Deleting local data

Use Sign out and Remove API key in Settings to remove AI credentials. Removing
`~/.config/flint` and `~/.local/share/flint` deletes the remaining Flint data.
The desktop keyring may also show entries labelled “Flint AI credential.”

## Contact

For privacy questions, open an issue on the project repository. Report security
issues through the private process in [SECURITY.md](SECURITY.md).
