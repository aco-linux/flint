# Privacy

Flint is a local-first desktop application. It has no Flint account, analytics,
advertising SDK, or maintainer-operated cloud service.

## Data kept on this computer

Flint stores settings under the standard user config directory and user data
(clips, notes, snippets, aliases, favorites, calc history, usage, learned
choices, per-app choice context, quicklinks, layouts) in `~/.local/share/flint/flint.db`. Private data
files are written with mode `600` inside directories with mode `700`. Learned
choices are query→item launch counts only (the text you typed and the result
id). They never leave this computer. **Clear learned choices** drops them.

## Focused window and clipboard context

When **Context-aware search** is on (the default), Flint reads the focused
Hyprland window’s class and title **once when the launcher opens**, before it
is presented, so Flint itself is never the recorded window. That snapshot
stays in memory for ranking and the empty-query chips. It is not written to
disk as a log.

If you launch a result while a class is captured, Flint may also store that
class next to the learned query in `choice_context` (`query`, `context_class`,
item id, count, last). Empty class is the existing global `choices` table.
**Clear learned choices** deletes both. Window titles are not stored there.

On an empty query, clipboard or primary text younger than 10 seconds can
appear as Paste / Search the web / Ask AI (and Open URL or Calc when they
apply). Secret-shaped clipboard text is skipped, same as the rest of Flint.

Turn **Context-aware search** off in Settings (`set:context`, or
`general.context_aware: false`) to skip the window snapshot, skip
`choice_context` writes, and hide the 10-second clipboard chips. Flint never
uses AT-SPI or inspects another app’s widgets.

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
