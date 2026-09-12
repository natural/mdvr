# Reload scenarios

Use `before.md` as existing content. Replace it atomically with `after.md`; then delete it; then recreate it with `before.md`. `empty.md` verifies empty-document state rather than stale content. Tests must control timing and assert generation/stale-result handling; fixture files contain no timing assumptions.
