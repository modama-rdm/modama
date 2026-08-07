See also: DEV.md

Use `justfile` commands when applicable; for example,
update-snapshots includes compression,
which can be easily forgotten on a manual run.

Many `cargo` commands can be run with `-q` to not clutter
the output with useless log messages. Errors are not hidden
by this, so it's a safe default.
