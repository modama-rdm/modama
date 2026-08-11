See also: DEV.md

## Tests and common tasks

Use `justfile` commands when applicable; for example,
update-snapshots includes compression,
which can be easily forgotten on a manual run.

Many `cargo` commands can be run with `-q` to not clutter
the output with useless log messages. Errors are not hidden
by this, so it's a safe default.

Fixing clippy errors is mandatory.

## Visual tests for GUI development

If at the end of a turn visual differences remain, do not clean them away,
rather keep them around for inspection and eventual acceptance.

In general, updating GUI snapshots should only be done after manual
inspection of the differences.
