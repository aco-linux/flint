#!/usr/bin/env bash
# Fast-forward this Flint checkout to origin, rebuild, install, restart the daemon.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

REMOTE="${FLINT_REMOTE:-origin}"
REF="${1:-}"
export PATH="${HOME}/.local/bin:${PATH}"

git fetch "$REMOTE"

if [[ -z "$REF" ]]; then
  if REF="$(git rev-parse --abbrev-ref --symbolic-full-name '@{u}' 2>/dev/null)"; then
    :
  else
    REF="${REMOTE}/HEAD"
  fi
fi

before="$(git rev-parse --short HEAD)"
want="$(git rev-parse --short "$REF")"
if [[ "$before" != "$want" ]]; then
  if ! git diff-index --quiet HEAD --; then
    echo "flint-update: working tree has local changes; commit or stash first" >&2
    git status -sb
    exit 1
  fi
  git merge --ff-only "$REF"
fi
after="$(git rev-parse --short HEAD)"

make install

if command -v flint >/dev/null 2>&1; then
  flint --quit >/dev/null 2>&1 || true
fi
for _ in 1 2 3 4 5 6 7 8 9 10; do
  pgrep -x flint >/dev/null 2>&1 || break
  sleep 0.15
done
if pgrep -x flint >/dev/null 2>&1; then
  pkill -x flint || true
  sleep 0.2
fi
if pgrep -x flint >/dev/null 2>&1; then
  echo "flint-update: old flint process still running" >&2
  pgrep -a -x flint >&2 || true
  exit 1
fi

nohup flint --daemon >/dev/null 2>&1 &
disown
sleep 0.4
if ! pgrep -x flint >/dev/null 2>&1; then
  echo "flint-update: daemon failed to start" >&2
  exit 1
fi

echo "Flint ${after} installed to $(command -v flint)"
if [[ "$before" != "$after" ]]; then
  echo "moved ${before} -> ${after} (${REF})"
else
  echo "already at ${after} (${REF})"
fi
pgrep -a -x flint
ls -la --time-style=long-iso "$(command -v flint)"
