# Changelog

## Unreleased — in-pane media (Wave F)

- Preview uses `gtk4::Video` + `MediaFile` when GStreamer is present at runtime (`dlopen` of `libgstreamer-1.0.so.0`). Flint does not link GStreamer. Without it, Space/Enter still use the default player.
- Space play/pauses in the pane for audio/video, or enlarges an image. Esc returns to the list with the same query and selection. Enter always opens the external player.
- `media.hide_on_external_play` defaults to **false** (Settings / `set:media-hide`). Query and selection restore on Hyprland `closewindow`, or `activewindow` when Flint is focused again.
- The window grows to `WINDOW_HEIGHT_MAX` (900) via `fit_window` while the media/image pane is open.

## Unreleased — real web results (Wave E)

- In-app web rows use DuckDuckGo HTML (`web.provider = "ddg-html"`). An Instant Answer, when present, is a bonus first row; HTML hits follow (cap 6); **Open DuckDuckGo** stays last. `searxng` and `brave` are documented fallbacks and currently use the same HTML path (no extra API keys).
- Fetch only on Web intent, a trailing `?` (not the Ask prefix `? `), or three-or-more words with no title-prefix fuzzy hit. A single-token app name such as `firefox` never starts a worker. 300 ms debounce; 60 s cache kept.
- `web.provider = "off"` (Settings, `set:web`) skips every DuckDuckGo request. See [PRIVACY.md](PRIVACY.md).

## Unreleased — inline answers (Wave D)

- Result rows show a right-aligned answer (first 120 characters) for calc, unit conversion, color, timezone, and weather. Color rows include a hex swatch. Enter copies calc/convert/color and stays in the launcher; the preview pane says so.
- Ask AI streams token-by-token into that answer slot via the existing generation-checked worker. Only **Ask intent**, `?`, or `ask ` (250 ms debounce). A new query cancels the in-flight stream. Enter still opens the full transcript.
- Weather live-fetches while the weather intent is showing, even if a cache exists. Clipboard ingest refreshes empty-query chips while Flint is open.
- Extension `List` with `onSearchTextChange` already pushes incremental renders; selection is kept across those updates.

## Unreleased — search context (Wave C)

- At show-time (before Flint is presented) the focused Hyprland window’s class and title are captured into `Context`. Root ranking adds +15k when a layout, quicklink, or snippet’s optional `app` field matches that class. Capture skips the launcher so Flint never records itself.
- Learned choices look up `(query, class)` in `choice_context`, then the global `(query, "")` row in `choices`. **Clear learned choices** drops both. `schema_version` stays `"1"`.
- Empty query, clipboard or primary selection younger than 10 seconds: Paste / Search the web / Ask AI, plus Open URL and Calc when they apply. An editor path in the window title (`foo.rs — Code`) seeds a recent-file row when that path exists.
- Toggle **Context-aware search** in Settings (and `set:context`). Default on. Off stores no window class and hides the 10s clipboard chips. Never AT-SPI. See [PRIVACY.md](PRIVACY.md).

## Unreleased — keystroke pipeline (Wave B)

- Root search no longer reads SQLite on every keystroke. Aliases, favorites, quicklinks, and custom layouts live in `Catalog` (`RefCell`) and load with `Catalog::load` / `reload_installed`. Action-panel alias, pin, quicklink, and layout mutators invalidate that cache.
- Ranking reads a precomputed `IndexEntry` (`title_lc`, words, initials, `keywords_lc` including alias, `subtitle_lc`). Alias changes rebuild that entry. The nucleo `Pattern` stays one-per-keystroke; a shared `Vec<char>` buffer is reused across items.
- Live rows (files, GIFs, calendar, mail, web) do not reorder what is already on screen. They insert at the scored index only when that index is at or below the current selection; otherwise they append. Weather stays at row 0 when the weather intent fired.
- `general.max_results` is the actual root mix cap. The hidden `.max(24)` floor is gone.
- Bench (ignored): `cargo test --bins --release -- --ignored --nocapture bench_root_search_p95` — 500-item pool, 50 queries. Release p50 **1.02 ms**, p95 **1.61 ms** (target p95 < 2 ms).

## Unreleased — root ranking (Wave A)

- Query→item memory: launching a root (or window) result records the typed query and its prefixes in `choices`. Next time that query is typed, the picked item is row 0 (115k, under an explicit alias). Prefixes (`s` after `sl` → Slack) get a smaller 20k boost. Not recorded from Files, Clipboard, or extension-internal actions. **Clear learned choices** (root + Settings, keyword `forget`) wipes the table. `FLINT_RANK_DEBUG=1` logs the top 10 scores.
- Title-first fuzzy rank: nucleo on title / keywords×0.6 / subtitle×0.3, take the max, divide by character length (min 4). Exact title +10k, prefix +4k, word/initials +2.5k, keyword +1.5k. Kind labels are no longer in the haystack. Same-day usage cannot overturn an exact title over a prefix; it can break a prefix tie.
- Fixed tiers only fire when the query means them. Calc at 100k needs an operator (`1` is not calc; `1+1` is). Memory/translate/tz/instant sit at 85k. Emoji only when the query looks like emoji (or `:shortcode:`) — never the old 3-row leak. Type-word queries (`video`, `type:pdf`) reserve row 0 for a files placeholder and cap apps below that tier. Weather is 110k on the first frame (cached summary or “Detecting…”), replaced in place when live data arrives. Intents stay at 70k so a learned `we` → WezTerm can outrank weather after two picks.
- Tie-break is last-used, then title. If you have moved off row 0, the selected item stays selected across keystrokes while it remains in the list.

## Unreleased — in-app results and smart sizing

- Natural-language intents: “what’s the weather”, “is it going to rain”, “what’s on my calendar today”, “what’s in my inbox” route to live cards instead of a web search
- Ask AI shows a scrollable transcript in the same window (not a truncated one-line row). Native tools read calendar, weather, iCloud inbox, and Instant Answers
- GIFs preview in the list; the Tenor browser fallback is gone
- DuckDuckGo Instant Answers render in Flint. Opening a SERP is an action-panel escape hatch
- Apple CalDAV discovers the calendar collection and filters today. Google/Outlook events copy in-app instead of opening HTML
- Light iCloud IMAP inbox using the same Apple app-specific password
- The floating window starts compact (~980×400) and grows with weather cards, agendas, GIFs, search snippets, and chat
- Empty-state chips and Ctrl+K action rows are clickable

## Unreleased — store and desktop glue (Wave 6)

- Extension `confirmAlert` shows Confirm / Cancel rows; Enter resolves the RPC (no longer cancel-always)
- `Form` is an honest list of fields: Enter edits with the clipboard-rename / TextView pattern and calls `onChange`; checkbox toggles; Submit is a row (`onSubmit` gets current values). Grid still renders as a list
- `getSelectedText` is `wl-paste --primary`, then clipboard — never AT-SPI. Job 88 (focused-app menu search) is not implemented
- Settings lists one row per installed extension (manifest defaults). Editing preferences is not implemented
- Optional Hyprland binds in `share/flint-binds.conf`, copied to `~/.config/hypr/flint-binds.conf` only if missing. Source it yourself. Caps Lock as Hyper: `share/keyd-hyper.conf` (keyd/kanata); Flint does not ship a remapper
- Idle-kill the extension Node process ~5 minutes after the last message. No extra timer when extensions are off
- Calendar, meetings, Notion, OpenClaw, typing: Store / extensions, not core

## Unreleased — AI threads, memory, skills (Wave 5)

- Threaded Ask AI in SQLite (`ai_threads` / `ai_messages`). First Ask creates a chat; follow-ups append user+assistant. Empty Ask lists recent chats (LIKE search on title/text). Enter resumes. Esc or **New chat** starts another
- Attachments: clipboard text (secrets skipped), selected file (Ctrl+K or a copied path), share screen / region — hide, capture once with grim/slurp, attach a local path plus optional tesseract OCR. Images are never uploaded
- Prompt templates on `{selection}` (primary, then clipboard): **Fix grammar**, **Quick Fix**, **Translate selection**, **Explain selection**
- Memory: **Remember …** / **Show memory** / **Forget …** as user-visible SQLite rows, injected into the system prompt (cap 2 KiB)
- Skills: `~/.config/flint/skills/*.md` appended to the system prompt (cap 12 KiB). Missing directory is fine
- MCP stays a prompt primer behind `allow_mcp`. Flint does not run model-chosen tools
- **Open OpenClaw** only if `openclaw` / `open-claw` is on PATH. The default local model name still mentions Hermes; that is not an agent runtime

## Unreleased — dictation, notes, focus (Wave 4)

- **Dictate to focused app:** hide Flint, record, transcribe; `wtype --` types the text as argv (never ydotool). Missing wtype copies and the status says to install it. In-bar dictation is unchanged; Ctrl+K **Paste with wtype** on a transcript
- Dictation history in SQLite (`dictation` table, cap 100). Voice mode empty query lists it; Enter pastes. Root command **Voice history**
- Language/style are Ask AI prompt templates on the last transcript (email, formal, concise, bullets, translate) — not a second STT engine
- **Note from selection:** `wl-paste --primary` (then clipboard). Empty primary → status. Job 29 is primary selection, not AT-SPI
- Focus timer: **Start focus 25m**, **Start break 5m**, **Stop focus**, **Unfocus**. glib 1s timeout only while a session is running. `status.json` (`{text,class,tooltip}`) for Waybar. Root shows remaining time from an in-process mutex, not a file poll

## Unreleased — in-bar content tools (Wave 3)

- Content search: explicit `content:` / `in:` (or Files mode `content <q>`). One cancellable `rg --max-count 1` over `$HOME` and `search_roots`, never `/`. Ordinary root typing stays in-memory
- Built-in emoji table (~200) with keywords and `:shortcode:`; `emoji ` mode; Enter pastes. Ranks above `omarchy-menu-emoji` when the query looks like emoji
- Timezones: static city table (`time in tokyo`, `nyc vs london`) with fixed UTC offsets. Subtitle says standard offset, not DST
- Colors: hex still instant; `rgb()` converts to hex + HSL; **Pick color** only if `hyprpicker` or `wl-color-picker` is on PATH
- Translate / define are Ask AI prompt templates (`tr fr hello`, `translate es …`, `en:de thanks`, `define widget`). No new HTTP. Currency skipped
- OCR (`tesseract`) and QR (`zbarimg`) as argv, only if found: action panel on image files, plus clipboard PNG via `wl-paste` into a mode-600 temp file that is deleted after

## Unreleased — lightness (Wave 2.5)

- User data (clips, notes, snippets, aliases, favorites, calc history, usage, quicklinks, layouts, quit-keep) lives in one `~/.local/share/flint/flint.db` (mode 600) with incremental writes
- Existing JSON files are imported once and renamed to `*.json.bak`; config, auth, and API keys stay JSON / Secret Service
- Snippets and quicklinks share one placeholder engine (`{clipboard}`, `{date}`, `{time}`, `{datetime}`, `{day}`, `{increment}`, `{cursor}`, `{argument}` / `{Query}`, `{selection}`)
- Clipboard ingest is GDK `changed` only — the 1s `wl-paste` poll is gone
- Unpinned clips are trimmed by size (512 KiB of UTF-8) and the existing row ceiling; pinned clips are kept

## Unreleased — windows and system (Wave 2)

- Window layouts: left/right/top/bottom halves, four quarters, maximize, center, almost-maximize (48px inset), next/previous display
- Custom named layouts in `~/.config/flint/layouts.json`; `layout +name` (or `win +name`) saves the current arrangement and applies by class
- Quit / force-quit a window or app; quit all except Flint and `~/.config/flint/quit-keep.json` (confirm step)
- Uninstall via Flatpak (`--user` preferred) or `pkexec pacman -Rns` when the desktop file maps to a package; unknown packages are not guessed
- Screenshot, region, record, and annotate commands wrap grim / slurp / wf-recorder / satty (or swappy); Omarchy capture helpers remain if grim is missing
- Display resolution commands from `hyprctl monitors` plus 720p/1080p/1440p/4K

## Unreleased — command-bar parity (Wave 1)

- Clipboard entries can be pinned, renamed, and edited; pinned clips survive the 80-item trim; paste-as-plain is in the action panel
- Aliases (nicknames) for any result: Ctrl+K → Set alias uses the action-filter text, or prompts with `alias:`
- Favorites pin to the top of an empty root list and get a ranking boost
- Snippet placeholders: `{clipboard}`, `{date}`, `{time}`, `{datetime}`, `{day}`, `{increment}`, `{cursor}`
- Date and percent answers next to the existing calculator (`today + 7d`, `days until YYYY-MM-DD`, `20% of 80`, `20% off 80`, `80 + 20%`)
- Calculation history (`calc` / `=`) stored privately; empty root does not dump it
- Quicklinks (`link`): URL, folder, or file targets with `{argument}` / `{Query}`; `+name url` creates; javascript: rejected
- Ctrl+K action panel (copy path, open with, show in files, pin, paste, set alias). Ctrl+? is Ask AI
- Throw confetti command (accent + cream overlay, ~1.2s)

## Unreleased — launcher map

- Stop the open animation that tiles Flint large then shrinks it: Hyprland now floats, sizes, and centers on the first frame (`no_anim`), and Flint no longer re-dispatches float/resize after map

- Opt-in Node host for installed Vicinae / Raycast-style extensions (`general.allow_extensions`, off by default). Each command is one Node process with real `react` 19 and `@vicinae/api`; List/Detail/Form-as-list rows render in Flint's result list
- Enter runs the first action, Shift+Enter the second; Esc pops a pushed view then leaves. Clipboard, open, terminal, LocalStorage, toasts, confirm, selected-text, and no-view commands work
- Runtime (`~/.local/share/flint/runtime/`) is installed with pinned `npm` packages on first launch; command sources are bundled with `esbuild`. Config is sent on stdin, not argv
- Not yet: Grid layout, menu-bar, extension OAuth, preference editing, or file-search RPC

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
