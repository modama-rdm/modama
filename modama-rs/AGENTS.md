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

## Integration tests

Run `just integration-tests` (see "Run the integration tests" in DEV.md) after
changing how the components talk to each other: smbdrop's HTTP API or event
channel, the agent's local API, or the agent's use of the management API. They
are not part of CI, so nothing else catches it when they break. Run them in the
background; the first start compiles the workspace in the containers.

The GUI's end-to-end tests (`modama-agent-gui/tests/e2e_*.rs`) run there too.
Add one only for what the unit and visual tests can't catch: a flow across the
GUI, the agent, the session helper and the services. One test per flow, with
few, telling checks; texts and layout belong in the visual tests.

A failure can come from the tests being out of date rather than from the change:
check what the test expects against the current behavior, and fix the test if
it is outdated. Say which tests you changed and why.

## Visual tests for GUI development

Run visual tests via `just`; otherwise, required workaround(s) for issues
in third-party libraries will not apply and the test run might crash.
Do **not** speculatively run with `--test-threads=1`.

If at the end of a turn visual differences remain, do not clean them away,
rather keep them around for inspection and eventual acceptance.
`just visual-tests` gathers them (`A.png`, `A.new.png`, `A.diff.png`) in
`modama-agent-gui/tests/snapshots/review/`; point the user there
(`just review-snapshots` regathers them).

Accepting snapshot differences is the user's decision. After a change that
alters snapshots, leave the review folder in place, tell the user which
differences to expect and where to look, and run `just update-snapshots`
(and `just clean`) only after they accepted. Looking at the review images
yourself is helpful, but it doesn't replace the user's acceptance.

A visual test that clicks, types or scrolls before a snapshot also asserts
the effect it expects, such as the dialog being open or a label being
there. Otherwise a click that misses still produces a snapshot, and its
difference looks like any other.

When reviewing differences, look at the snapshots with the largest pixel
difference first (the test output reports each one's size); don't conclude
from a sample.

CI's images of a pull request's failing visual tests are on the branch
`ci-snapshot-review`, under `pr-<number>/`.

## Shape of UI changes

A small change, such as a text, a label or an icon next to an existing
element, can be agreed in words. Anything bigger is agreed on its shape
before it is built: a new dialog, page or major UI element, or a layout
change that alters most snapshots.

1. Show a mockup first (an ASCII sketch, for example as previews in a
   question) and agree on it.
2. As the first step of the implementation, write the visual test for the
   new UI, run only that test with `just visual-test <name>`, and show the
   user the snapshots from `modama-agent-gui/tests/snapshots/review/`.
   If the change affects several pages or states, show a representative
   snapshot of each. Skip this step if the user asked you to implement something on your own.

Only then build out the rest.

## GUI layout

Read "Layout › Guides" in `mgmtinterface-new/DESIGN.md` before placing
anything in the GUI. Positions on the guides come from the guide helpers in
`modama-agent-gui/src/styles.rs` (`outer_guide_inset`, `inner_guide_inset`,
`guide_inset`, `bottom_overlay_inset`), never from new numbers. Every new
element placed on a guide gets a geometry test, like
`footer_version_sits_on_the_inner_right_guide` in
`modama-agent-gui/tests/overview_visual.rs`.

## Dependencies

When adding native dependencies (for example via `*-sys` packages), especially those
that cannot be linked in statically, not only is this choice that needs discussion and
explicit consent - it also needs updates in different places:

- CI test job(s)
- CI build job(s)
- Containers, if any
- integration test environments

These might need to use system package managers to install the required libraries.
