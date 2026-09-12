# Symlink cases

`escape.md` points outside `tests/fixtures` to repository `docs/design.md`; resource policy must canonicalize before allowing reads. `linked-documents` points at `../documents`; discovery must not traverse symlink directories. These links are intentional and must remain symlinks.
