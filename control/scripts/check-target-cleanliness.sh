#!/usr/bin/env bash
set -euo pipefail

target="${1:-/workspace/product/target}"

fail() {
  echo "ERROR: $1" >&2
  exit 1
}

test -d "$target" || fail "target workspace not found: $target"

test ! -d "$target/.agents" || fail "target repo must not contain .agents/"
test ! -d "$target/.devcontainer" || fail "target repo must not contain .devcontainer/"

for path in prompts memory reviews ai-memory scratch; do
  test ! -e "$target/$path" || fail "target repo must not contain $path"
done

if rg -n \
  "control harness|control repo|/workspace/product/control|owox-harness-control" \
  "$target" \
  --glob '!target/**' \
  --glob '!tests/fixtures/**/.owox/harness/**' \
  >/tmp/owox-harness-control-contamination.txt; then
  cat /tmp/owox-harness-control-contamination.txt >&2
  fail "target repo contains control harness context"
fi

# target 直下の .owox は生成済み target harness なので許可する。
# 迷子の nested .owox (root でも tests/fixtures でもない) だけを弾く。
if find "$target" -type d -name .owox \
  ! -path "$target/.owox" \
  ! -path "$target/tests/fixtures/*/.owox" \
  ! -path "$target/tests/fixtures/*/*/.owox" \
  ! -path "$target/tests/fixtures/*/*/*/.owox" \
  -print -quit | rg . >/tmp/owox-harness-control-bad-owox.txt; then
  cat /tmp/owox-harness-control-bad-owox.txt >&2
  fail ".owox is only allowed at the target root (generated harness) or under tests/fixtures/**/.owox"
fi

if find "$target/tests/fixtures" -type d -name .owox 2>/dev/null \
  -print | while read -r dir; do test -d "$dir/harness" || echo "$dir"; done \
  | rg . >/tmp/owox-harness-control-bad-fixtures.txt; then
  cat /tmp/owox-harness-control-bad-fixtures.txt >&2
  fail "fixture .owox directories must contain .owox/harness/"
fi

echo "target repo cleanliness check passed"
