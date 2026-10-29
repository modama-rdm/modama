import type {SerializedProject} from '../../api/types'

/**
 * 24 of the real production project list's 227 entries, scrubbed (see README.md
 * in this directory). Selected to preserve the real spread: member counts 1-14,
 * name lengths 3-31 chars, projects owned by a bare numeric UID ("90000001",
 * "90000002" — the unresolved-username edge case), and one project ("mws")
 * whose sole member is its own owner.
 */
export const FIXTURE_PROJECTS: SerializedProject[] = [
    {
        "ctime": 1783695300.3267505,
        "description": "# Data management dogfooding\n\nTest bed for the **ER-C data management** tools.\n\n- Sessions from the agent and the SMB drop\n- See the [handbook](https://example.org/rdm-handbook)",
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
        "usage_bytes": null
    },
    {
        "ctime": 1707488642.9898474,
        "gid": 1020,
        "members": [
            "user12"
        ],
        "mtime": 1707488642.9898474,
        "name": "ou233776_mo_b1_ftbvksy",
        "owner": "user12",
        "relpath": "hdtms/ou233776_mo_b1_ftbvksy",
        "usage_bytes": null
    },
    {
        "ctime": 1681305028.387378,
        "gid": 1020,
        "members": [
            "user6",
            "user5",
            "user13",
            "user1",
            "user14"
        ],
        "mtime": 1681305028.387378,
        "name": "wogx",
        "owner": "user1",
        "relpath": "hdtms/wogx",
        "usage_bytes": null
    },
    {
        "ctime": 1648048920.6235373,
        "gid": 1020,
        "members": [
            "user15"
        ],
        "mtime": 1648048920.6235373,
        "name": "qgkzv_5820",
        "owner": "user15",
        "relpath": "hdtms/qgkzv_5820",
        "usage_bytes": null
    },
    {
        "ctime": 1648053026.5132246,
        "gid": 1020,
        "members": [
            "90000001",
            "user16",
            "user17",
            "user18"
        ],
        "mtime": 1648053026.5132246,
        "name": "yekr",
        "owner": "90000001",
        "relpath": "hdtms/yekr",
        "usage_bytes": null
    },
    {
        "ctime": 1677245359.0605576,
        "gid": 1020,
        "members": [
            "user1",
            "user3",
            "user6"
        ],
        "mtime": 1677245359.0605576,
        "name": "ecspzl-u-aidtht",
        "owner": "user6",
        "relpath": "hdtms/ecspzl-u-aidtht",
        "usage_bytes": null
    },
    {
        "ctime": 1654858842.8907938,
        "gid": 1020,
        "members": [
            "user1",
            "user15",
            "user6"
        ],
        "mtime": 1654858842.8907938,
        "name": "zpxe-wqb",
        "owner": "user15",
        "relpath": "hdtms/zpxe-wqb",
        "usage_bytes": null
    },
    {
        "ctime": 1660562632.411734,
        "description": "Off-axis holography of Ni nanowires\nTitan G2, 300 kV, Feb 2026",
        "gid": 1020,
        "members": [
            "user19",
            "user20"
        ],
        "mtime": 1660562632.411734,
        "name": "wys_ztv0",
        "owner": "user19",
        "relpath": "hdtms/wys_ztv0",
        "usage_bytes": null
    },
    {
        "ctime": 1674811601.3398495,
        "gid": 1020,
        "members": [
            "user21",
            "user10"
        ],
        "mtime": 1674811601.3398495,
        "name": "drl_wu6j7_72wk",
        "owner": "user21",
        "relpath": "hdtms/drl_wu6j7_72wk",
        "usage_bytes": null
    },
    {
        "ctime": 1726252101.4901943,
        "gid": 1020,
        "members": [
            "90000002"
        ],
        "mtime": 1726252101.4901943,
        "name": "xlzyki",
        "owner": "90000002",
        "relpath": "hdtms/xlzyki",
        "usage_bytes": null
    },
    {
        "ctime": 1699523424.697907,
        "description": "Lorentz TEM of skyrmion lattices.",
        "gid": 1020,
        "members": [
            "user6",
            "user22",
            "user7",
            "user23",
            "user24",
            "user1"
        ],
        "mtime": 1699523424.697907,
        "name": "fzhogcatlvnwg",
        "owner": "user1",
        "relpath": "hdtms/fzhogcatlvnwg",
        "usage_bytes": null
    },
    {
        "ctime": 1723279486.7989495,
        "gid": 1020,
        "members": [
            "90000002"
        ],
        "mtime": 1723279486.7989495,
        "name": "ztr",
        "owner": "90000002",
        "relpath": "hdtms/ztr",
        "usage_bytes": null
    },
    {
        "ctime": 1669887499.1991034,
        "gid": 1020,
        "members": [
            "user25"
        ],
        "mtime": 1669887499.1991034,
        "name": "gwqzntxu_gyhxphflzf_qm_fj_m_cg",
        "owner": "user25",
        "relpath": "hdtms/gwqzntxu_gyhxphflzf_qm_fj_m_cg",
        "usage_bytes": null
    },
    {
        "ctime": 1738830592.703841,
        "gid": 1020,
        "members": [
            "user26",
            "user27",
            "user6",
            "90000003",
            "90000004",
            "user23",
            "user1",
            "user28",
            "user29",
            "user30"
        ],
        "mtime": 1738830592.703841,
        "name": "auypfzdy",
        "owner": "user1",
        "relpath": "hdtms/auypfzdy",
        "usage_bytes": null
    },
    {
        "ctime": 1695647529.8881881,
        "gid": 1020,
        "members": [
            "user31",
            "90000005",
            "user6"
        ],
        "mtime": 1695647529.8881881,
        "name": "brl_qswpdhj_aeewkoc",
        "owner": "user31",
        "relpath": "hdtms/brl_qswpdhj_aeewkoc",
        "usage_bytes": null
    },
    {
        "ctime": 1782992682.724595,
        "gid": 1020,
        "members": [
            "user15",
            "user6",
            "user32",
            "user33",
            "user34",
            "user35",
            "user1",
            "user11"
        ],
        "mtime": 1782992682.724595,
        "name": "eiidjlv-sywdhdjdnp",
        "owner": "user15",
        "relpath": "hdtms/eiidjlv-sywdhdjdnp",
        "usage_bytes": null
    },
    {
        "ctime": 1725370023.7970824,
        "gid": 1020,
        "members": [
            "user36",
            "user6",
            "user7",
            "90000006",
            "user19",
            "user1"
        ],
        "mtime": 1725370023.7970824,
        "name": "rhjksukhr",
        "owner": "user1",
        "relpath": "hdtms/rhjksukhr",
        "usage_bytes": null
    },
    {
        "ctime": 1706632326.5486073,
        "gid": 1020,
        "members": [
            "user12"
        ],
        "mtime": 1706632326.5486073,
        "name": "io473554_qyh_lxnj_paqyd37_4_wwh",
        "owner": "user12",
        "relpath": "hdtms/io473554_qyh_lxnj_paqyd37_4_wwh",
        "usage_bytes": null
    },
    {
        "ctime": 1784555723.8881893,
        "gid": 1020,
        "members": [
            "user37",
            "user38",
            "user6"
        ],
        "mtime": 1784555723.8881893,
        "name": "wic_lvfy",
        "owner": "user37",
        "relpath": "hdtms/wic_lvfy",
        "usage_bytes": null
    },
    {
        "ctime": 1784282966.4226284,
        "gid": 1020,
        "members": [
            "user39"
        ],
        "mtime": 1784282966.4226284,
        "name": "mws",
        "owner": "user39",
        "relpath": "hdtms/mws",
        "usage_bytes": null
    },
    {
        "ctime": 1780497915.4132433,
        "gid": 1020,
        "members": [
            "user2",
            "user4",
            "user15",
            "user6",
            "user40",
            "user41",
            "user33",
            "user19",
            "user35",
            "user42",
            "user1",
            "user20",
            "user29",
            "user11"
        ],
        "mtime": 1780046409.3210287,
        "name": "ljfmguujxbnwc",
        "owner": "user1",
        "relpath": "hdtms/ljfmguujxbnwc",
        "usage_bytes": null
    },
    {
        "ctime": 1730313064.0470185,
        "gid": 1020,
        "members": [
            "user43"
        ],
        "mtime": 1730313064.0470185,
        "name": "sqmpnh_62449",
        "owner": "user43",
        "relpath": "hdtms/sqmpnh_62449",
        "usage_bytes": null
    },
    {
        "ctime": 1730885947.227222,
        "gid": 1020,
        "members": [
            "user31",
            "user44",
            "user38",
            "user6"
        ],
        "mtime": 1730885947.227222,
        "name": "yeknnhdy-wqpdhtfl-ncgq-jrdkkqev",
        "owner": "user6",
        "relpath": "hdtms/yeknnhdy-wqpdhtfl-ncgq-jrdkkqev",
        "usage_bytes": null
    },
    {
        "ctime": 1736862577.731927,
        "gid": 1020,
        "members": [
            "user6",
            "user37",
            "user25",
            "user39",
            "user38"
        ],
        "mtime": 1736862577.731927,
        "name": "vxb-i667",
        "owner": "user39",
        "relpath": "hdtms/vxb-i667",
        "usage_bytes": null
    }
]
