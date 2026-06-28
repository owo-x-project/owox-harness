#!/usr/bin/env bash
set -euo pipefail

mkdir -p /workspace/product/target
mkdir -p /root/.codex /root/.claude /root/.claude-state /root/.config/anthropic /root/.npm-global/bin

if test -L /root/.claude.json; then
  :
elif test -f /root/.claude.json; then
  mv /root/.claude.json /root/.claude-state/claude.json
  ln -s /root/.claude-state/claude.json /root/.claude.json
else
  touch /root/.claude-state/claude.json
  ln -s /root/.claude-state/claude.json /root/.claude.json
fi

cp /tmp/host.gitconfig /root/.gitconfig || true
git config --global --add safe.directory /workspace/product
git config --global --add safe.directory /workspace/product/target



if ! command -v codex >/dev/null 2>&1; then
  OWOX_RTK_SHIM=0 npm install -g @openai/codex
fi

if command -v rtk >/dev/null 2>&1; then
  rtk telemetry disable || true
  rtk init -g --codex --auto-patch || rtk init -g --codex
fi

if ! command -v claude >/dev/null 2>&1; then
  OWOX_RTK_SHIM=0 npm install -g @anthropic-ai/claude-code
fi


test -f /workspace/product/control/.owox-version || {
  echo "ERROR: /workspace/product/control/.owox-version is missing" >&2
  exit 1
}

OWOX_VERSION="$(tr -d '\r\n' </workspace/product/control/.owox-version)"
test -n "$OWOX_VERSION" || {
  echo "ERROR: /workspace/product/control/.owox-version is empty" >&2
  exit 1
}

OWOX_VERSION="$OWOX_VERSION" bash /workspace/product/control/scripts/setup.sh

echo "owox-harness control harness is ready"
