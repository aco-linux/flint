# Contributing

Flint is a public native Linux application. Small, reviewable patches beat large “make it Raycast” PRs.

## Setup

```sh
cargo test
cargo build --release
```

You need GTK4 development headers and `pkg-config`.

## Rules

- Stay honest in the UI and README. Do not claim Raycast store compatibility, Form/Grid/OAuth for extensions, or a shared OAuth client.
- Default-deny anything that executes third-party code.
- Private files go through `paths::write_private` (mode 600).
- Tokens never go on process argv and must remain bound to their provider/API origin.
- Add a unit test when you change parsing, allow-lists, or secret heuristics.

## Commit style

Conventional, short: `fix: …`, `feat: …`, `docs: …`, `security: …`.
