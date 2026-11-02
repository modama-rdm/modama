See also: DEV.md

## Tests and common tasks

Use `justfile` commands when applicable; for example,
update-snapshots includes compression,
which can be easily forgotten on a manual run.

Many `cargo` commands can be run with `-q` to not clutter
the output with useless log messages. Errors are not hidden
by this, so it's a safe default.

Fixing clippy errors is mandatory.

Start integration test files (`tests/*.rs`, including the visual tests) with
`#![cfg(test)]`: otherwise clippy doesn't count their helper functions as test
code, and `allow-unwrap-in-tests` doesn't apply to them.

## Visual tests for GUI development

Run visual tests via `just`; otherwise, required workaround(s) for issues
in third-party libraries will not apply and the test run might crash.
Do **not** speculatively run with `--test-threads=1`.

If at the end of a turn visual differences remain, do not clean them away,
rather keep them around for inspection and eventual acceptance.

In general, updating GUI snapshots should only be done after manual
inspection of the differences.

## Dependencies

When adding native dependencies (for example via `*-sys` packages), especially those
that cannot be linked in statically, not only is this choice that needs discussion and
explicit consent - it also needs updates in different places:

- CI test job(s)
- CI build job(s)
- Containers, if any
- integration test environments

These might need to use system package managers to install the required libraries.
