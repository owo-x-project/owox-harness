#!/usr/bin/env bash
set -euo pipefail

test -d /workspace/product/control || {
  echo "ERROR: /workspace/product/control is missing" >&2
  exit 1
}

test ! -d /workspace/product/control/.owox || {
  echo "ERROR: /workspace/product/control/.owox must not exist" >&2
  exit 1
}

mkdir -p /workspace/product/target

git config --global --add safe.directory /workspace/product
git config --global --add safe.directory /workspace/product/target

test -d /workspace/product/target || {
  echo "ERROR: /workspace/product/target is missing" >&2
  exit 1
}

if test ! -d /workspace/product/target/.git; then
  echo "WARNING: /workspace/product/target does not look like a git repository yet" >&2
  echo "Clone or mount the target repo into /workspace/product/target before target harness work." >&2
else
  bash /workspace/product/control/scripts/check-target-cleanliness.sh /workspace/product/target
fi

if ! command -v codex >/dev/null 2>&1; then
  npm install -g @openai/codex
fi

if command -v rtk >/dev/null 2>&1; then
  rtk telemetry disable || true
  rtk init -g --codex --auto-patch || rtk init -g --codex
fi

if ! command -v claude >/dev/null 2>&1; then
  npm install -g @anthropic-ai/claude-code
fi

if ! command -v opencode >/dev/null 2>&1; then
  npm install -g opencode-ai
fi

test -f /workspace/product/owox-harness.code-workspace || {
  echo "ERROR: /workspace/product/owox-harness.code-workspace is missing" >&2
  exit 1
}

echo "owox-harness control harness is ready"
