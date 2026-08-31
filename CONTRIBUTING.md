# Contributing

Flint is public 0.1. Small, reviewable patches beat large “make it Raycast” PRs.

## Setup

```sh
cargo test
cargo build --release
```

You need GTK4 and gtk4-layer-shell headers.

## Rules

- Stay honest in the UI and README. Do not claim a JS extension host or a shared OAuth client.
- Default-deny anything that executes third-party code.
- Private files go through `paths::write_private` (mode 600).
- Tokens never on process argv.
- Add a unit test when you change parsing, allow-lists, or secret heuristics.

## Commit style

Conventional, short: `fix: …`, `feat: …`, `docs: …`, `security: …`.
