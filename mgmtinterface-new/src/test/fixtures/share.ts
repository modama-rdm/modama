import type {IngestShareV2} from '../../api/types'

/**
 * A real ingest share, captured from the UTM test VM (2026-07-22) via
 * `scripts/fixture-capture/capture-share.sh` — create, capture, delete, so
 * nothing lingers. Scrubbed (see `test/fixtures/README.md`): the project
 * segment of `dest_path` is replaced through the same seeded, non-reversible
 * substitution as the other fixtures; the dataset segment is kept verbatim
 * (session names aren't identities). `username`/`password`/`uid` are
 * server-generated per-share secrets for a share that no longer exists — kept
 * shape-only (not the literal captured values) out of hygiene, not because
 * they're still live.
 *
 * This corrected two guesses in the original hand-written placeholder:
 * `dest_path` is actually rooted at `/data/adhoc/...` (not `/srv/smb/data/...`),
 * and `share_name` is actually `share-<share_id>` (not an independent name).
 *
 * The smbdrop fields are the capture; `project`, `session`, `smb_host` and
 * `workgroup` are what the new API adds on top (#172), here with the values
 * it would derive from this `dest_path` and the defaults it ships with.
 */
export function makeFixtureShare(overrides: Partial<IngestShareV2> = {}): IngestShareV2 {
    const shareId = 'fx7c5c09fc-84a9-4d6a-a854-517f774fbd91'
    return {
        resource: null,
        share_id: shareId,
        share_name: `share-${shareId}`,
        dest_path: '/data/adhoc/hlhk_iapxbtw_mkgke/create by hand',
        created_timestamp: 1784720092,
        username: 'sharefx7c17ea405b19a44ec1be5630',
        password: 'FqXfx6xlyiodfTEMPYbLQmXNPTEMP01',
        uid: 90032240,
        gid: 1020,
        project: 'hlhk_iapxbtw_mkgke',
        session: 'create by hand',
        smb_host: 'moellenstedt.iff.kfa-juelich.de',
        workgroup: 'ER-C-INGEST',
        ...overrides,
    }
}
