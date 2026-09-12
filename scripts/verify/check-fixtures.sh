#!/bin/sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
fixtures="$root/tests/fixtures"

for path in \
 documents/rendering.md \
 documents/assets/one.png documents/assets/two.jpg documents/assets/three.gif \
 documents/assets/four.webp documents/assets/five.svg \
 security/hostile.md security/hostile.svg security/malformed.md \
 links/target.md reload/before.md reload/after.md reload/empty.md; do
 test -f "$fixtures/$path" || {
  printf 'missing: %s\n' "$path" >&2
  exit 1
 }
done

test -L "$fixtures/symlink-cases/escape.md"
test -L "$fixtures/symlink-cases/linked-documents"
test "$(wc -c <"$fixtures/reload/empty.md")" -eq 0
test "$(find "$fixtures/documents" -type f \( -name '*.md' -o -name '*.svg' \) | wc -l | tr -d ' ')" -ge 2
printf 'fixture check: passed (%s)\n' "$fixtures"
