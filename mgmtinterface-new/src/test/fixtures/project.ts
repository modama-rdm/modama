import type {SerializedDataset, SerializedProject} from '../../api/types'

/**
 * The real "datamanagement" project (Dieter's dogfooding project — 11 members,
 * 2.2 TB usage_bytes), scrubbed (see README.md). Session/dataset names are kept
 * verbatim: they're demo/test data ("create by hand", "testsession", …), not
 * identities, and are exactly the kind of value #202's quote_path parity exists
 * to round-trip correctly (spaces, digits, hyphens).
 *
 * Captured from the legacy details endpoint, which computed `usage_bytes`
 * inline. The new API answers the details with `usage_bytes: null` and the
 * size from `GET .../usage`; `world.ts` splits the fixture accordingly.
 */
export const FIXTURE_PROJECT: SerializedProject = {
    "ctime": 1783695300.3267505,
    "gid": 1020,
    "members": [
        "user2",
        "user3",
        "user4",
        "user5",
        "user6",
        "user7",
        "user8",
        "user9",
        "user10",
        "user1",
        "user11"
    ],
    "mtime": 1777293335.548407,
    "name": "vjfgcwmvztbiwm",
    "owner": "user1",
    "relpath": "hdtms/vjfgcwmvztbiwm",
    "usage_bytes": 2226346428528
}

export const FIXTURE_DATASETS: SerializedDataset[] = [
    {
        "ctime": 1783695300.3417509,
        "immutable": false,
        "mtime": 1707831700.3981373,
        "name": "2022-03-22 my first session"
    },
    {
        "ctime": 1783695300.3417509,
        "immutable": false,
        "mtime": 1698334851.26475,
        "name": "create by hand"
    },
    {
        "ctime": 1783695300.342751,
        "immutable": true,
        "mtime": 1648025216.3986871,
        "name": "demo session"
    },
    {
        "ctime": 1783695300.342751,
        "immutable": false,
        "mtime": 1648046120.3487308,
        "name": "session from smb"
    },
    {
        "ctime": 1783695300.342751,
        "immutable": false,
        "mtime": 1648046324.0693188,
        "name": "from ingest"
    },
    {
        "ctime": 1783695300.342751,
        "immutable": false,
        "mtime": 1648803265.1409917,
        "name": "2022-04-01 Debug HDF5 issues"
    },
    {
        "ctime": 1783695300.342751,
        "immutable": true,
        "mtime": 1772035061.9952013,
        "name": "testsession"
    },
    {
        "ctime": 1783695300.342751,
        "immutable": false,
        "mtime": 1654171654.2335906,
        "name": "2022-06-02 another testsession"
    },
    {
        "ctime": 1783695300.342751,
        "immutable": true,
        "mtime": 1655200260.946706,
        "name": "2022-06-14-datendemo"
    },
    {
        "ctime": 1783695300.343751,
        "immutable": false,
        "mtime": 1655200401.3175704,
        "name": "2022-06-14-test-from-ingest-gui"
    },
    {
        "ctime": 1783695300.343751,
        "immutable": false,
        "mtime": 1699281581.2950838,
        "name": "2023-11-06-ingest-stuff"
    },
    {
        "ctime": 1783695300.343751,
        "immutable": false,
        "mtime": 1700063376.684693,
        "name": "2023-11-15-dld-holo-vortex-test"
    },
    {
        "ctime": 1783695300.343751,
        "immutable": false,
        "mtime": 1700762913.1923795,
        "name": "2023-11-23-gms-sdk-testing"
    },
    {
        "ctime": 1783695300.3447511,
        "immutable": false,
        "mtime": 1705074313.2789896,
        "name": "2024-01-12-this-is-a-session"
    },
    {
        "ctime": 1783695300.3447511,
        "immutable": false,
        "mtime": 1731432810.0879617,
        "name": "2024-11-12-biasing-test"
    },
    {
        "ctime": 1783695300.3567514,
        "immutable": false,
        "mtime": 1772041519.3613904,
        "name": "2026-02-23-rdm-challenge-modama-agent"
    }
]
