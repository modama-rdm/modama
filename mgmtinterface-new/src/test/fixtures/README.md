# Journey-test fixtures

`projects.ts`, `project.ts`, `users.ts`, `browse.ts` are a scrubbed subset of real
`GET` responses captured from the ER-C production API (`er-c-data-mgmt.fz-juelich.de`)
on 2026-07-22, chosen to preserve the real variety of shapes the six journeys hit:
member-count spread (1–14), project-name length range (3–31 chars, letters/digits/
`_`/`-`), a numeric-UID-as-owner edge case (unresolved Unix UID instead of a
username), and folder names with spaces (`IT-Budget Call`) — the exact class of
value that `quotePath.ts` / #202 exists to handle correctly.

**Every real username, UID, and project/folder name has been replaced** with a
deterministic pseudo-random substitution of the same length and character class
(see `../../scripts/fixture-capture/`, which regenerates all of this). It is not
a public/reversible cipher (not ROT13): the substitution is seeded per-string,
so there's no known way back from a fake value to the real one without the
seed. Session/dataset names (`"create by hand"`, `"testsession"`, …) were kept
verbatim — they're demo/test data from dogfooding, not identities.

`share.ts` is a real capture from the UTM test VM (2026-07-22, prod itself had
no active shares to capture) — created, captured, and deleted via
`capture-share.sh` so nothing lingers. It corrected two guesses a first,
hand-written draft had gotten wrong: `dest_path` is rooted at `/data/adhoc/...`
(not `/srv/smb/data/...`), and `share_name` is `share-<share_id>` (not an
independent name).

If the FastAPI rewrite (#172) changes response shapes, these are what need
updating — the journeys themselves describe user-visible behavior and shouldn't
need to change. See `../../scripts/fixture-capture/README.md` for the full
capture -> scrub -> write workflow.

`browse.ts` was re-shaped by script from the 2026-07-22 capture when the data
browser moved to `GET /mgmt-api/browse` (#90): same entries, new field names,
sorted as the new API sorts. The capture scripts still read the legacy
`/api/browse/`; a fresh capture of the new endpoint needs the cookie login
rather than the Basic token and is left for when the next capture is due.
