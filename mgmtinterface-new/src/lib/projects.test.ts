import {describe, expect, it} from 'vitest'
import type {IngestShareV2, SerializedDataset, SerializedProject} from '../api/types'
import {
    availableMembers,
    canonicalProjectPath,
    filterProjects,
    filterSessions,
    findDatasetShare,
    groupProjects,
    memberChanges,
    PROJECT_SORT,
    SESSION_SORT,
    sharedDatasetNames,
    sortProjects,
    sortSessions,
} from './projectsUtil.ts'
import {nextSort} from './sort'

const p = (
    name: string,
    owner: string,
    members: string[],
    mtime: number,
): SerializedProject => ({
    name,
    owner,
    members,
    mtime,
    ctime: mtime,
    relpath: `data/${name}`,
    gid: 1020,
    usage_bytes: null,
})

const projects: SerializedProject[] = [
    p('datamanagement', 'weber', ['shanker', 'landers'], 1700000003),
    p('ghost_imaging', 'clausen', ['bangun', 'ganeva'], 1700000001),
    p('shanker_project', 'shanker', ['landers'], 1700000002),
    p('unrelated', 'bryan', ['amueller'], 1700000000),
]

describe('groupProjects', () => {
    it('partitions every project exactly once (own + member + other = total)', () => {
        const g = groupProjects(projects, 'weber')
        expect(g.own.length + g.member.length + g.other.length).toBe(projects.length)
    })

    it('puts owned projects in own, membership-only in member, rest in other', () => {
        const g = groupProjects(projects, 'weber')
        expect(g.own.every((p) => p.owner === 'weber')).toBe(true)
        expect(
            g.member.every((p) => p.owner !== 'weber' && p.members.includes('weber')),
        ).toBe(true)
        expect(
            g.other.every((p) => p.owner !== 'weber' && !p.members.includes('weber')),
        ).toBe(true)
        expect(g.own.some((p) => p.name === 'datamanagement')).toBe(true)
    })

    it('classifies a member-but-not-owner correctly', () => {
        const g = groupProjects(projects, 'shanker')
        expect(g.member.some((p) => p.name === 'datamanagement')).toBe(true)
        expect(g.member.every((p) => p.members.includes('shanker'))).toBe(true)
        expect(g.own.every((p) => p.owner === 'shanker')).toBe(true)
    })
})

describe('filterProjects', () => {
    const names = (ps: SerializedProject[]) => ps.map((p) => p.name)

    it('returns all projects for an empty query', () => {
        expect(filterProjects(projects, '')).toHaveLength(projects.length)
        expect(filterProjects(projects, '  ')).toHaveLength(projects.length)
    })

    it('matches names case-insensitively', () => {
        const result = filterProjects(projects, 'GHOST')
        expect(result.length).toBeGreaterThan(0)
        expect(result.every((p) => p.name.toLowerCase().includes('ghost'))).toBe(true)
        expect(result.some((p) => p.name === 'ghost_imaging')).toBe(true)
    })

    it('finds a hyphenated or underscored name by its words (#113)', () => {
        expect(names(filterProjects(projects, 'ghost imaging'))).toEqual(['ghost_imaging'])
        expect(names(filterProjects(projects, 'ghost-imaging'))).toEqual(['ghost_imaging'])
        expect(names(filterProjects(projects, 'shanker project'))).toEqual(['shanker_project'])
    })

    it('finds a name by two words in either order', () => {
        expect(names(filterProjects(projects, 'imaging ghost'))).toEqual(['ghost_imaging'])
        expect(names(filterProjects(projects, 'project shank'))).toEqual(['shanker_project'])
    })

    it('matches the owner too, each word on its own field', () => {
        expect(names(filterProjects(projects, 'clausen'))).toEqual(['ghost_imaging'])
        expect(names(filterProjects(projects, 'weber data'))).toEqual(['datamanagement'])
        // "shanker" is one project's owner and in another's name.
        expect(names(filterProjects(projects, 'shanker'))).toEqual(['shanker_project'])
    })

    it('does not match members or the path', () => {
        expect(filterProjects(projects, 'landers')).toEqual([])
        expect(filterProjects(projects, 'data/ghost')).toEqual([])
    })

    it('returns nothing when a word matches nowhere', () => {
        expect(filterProjects(projects, 'ghost nope')).toEqual([])
        expect(filterProjects(projects, 'zzz')).toEqual([])
    })

    it('puts a name that starts with the query before one that only contains it', () => {
        const ps = [p('my-demo', 'a', [], 1), p('demo-holo', 'a', [], 1), p('demo_x', 'a', [], 1)]
        expect(names(filterProjects(ps, 'demo'))).toEqual(['demo-holo', 'demo_x', 'my-demo'])
    })
})

describe('filterSessions', () => {
    const d = (name: string): SerializedDataset => ({name, ctime: 1, mtime: 1, immutable: false})
    const names = (ds: SerializedDataset[]) => ds.map((x) => x.name)
    const sessions = [
        d('2026-10-14 alignment run'),
        d('v1.2 calibration'),
        d('2026-09-30 4D-STEM map 01'),
        d('alignment-run-2'),
    ]

    it('returns all sessions for an empty query', () => {
        expect(filterSessions(sessions, '')).toHaveLength(sessions.length)
        expect(filterSessions(sessions, '  ')).toHaveLength(sessions.length)
    })

    it('finds a name by its words, dash or space alike (#113)', () => {
        expect(names(filterSessions(sessions, 'stem map'))).toEqual(['2026-09-30 4D-STEM map 01'])
        expect(names(filterSessions(sessions, '4d-stem'))).toEqual(['2026-09-30 4D-STEM map 01'])
        // "alignment-run-2" starts with the query, so it comes first.
        expect(names(filterSessions(sessions, 'alignment run'))).toEqual([
            'alignment-run-2', '2026-10-14 alignment run',
        ])
        expect(names(filterSessions(sessions, 'v1 2'))).toEqual(['v1.2 calibration'])
    })

    it('finds a name by two words in either order, regardless of case', () => {
        expect(names(filterSessions(sessions, 'MAP stem'))).toEqual(['2026-09-30 4D-STEM map 01'])
        expect(names(filterSessions(sessions, 'run 2026'))).toEqual(['2026-10-14 alignment run'])
    })

    it('puts a name that starts with the query first', () => {
        expect(names(filterSessions(sessions, 'alignment'))).toEqual([
            'alignment-run-2', '2026-10-14 alignment run',
        ])
    })

    it('returns nothing when a word matches nowhere', () => {
        expect(filterSessions(sessions, 'alignment nope')).toEqual([])
        expect(filterSessions(sessions, 'zzz')).toEqual([])
    })
})

describe('sortProjects', () => {
    const names = (ps: SerializedProject[]) => ps.map((p) => p.name)

    it('starts with the newest mtime first', () => {
        expect(names(sortProjects(projects, PROJECT_SORT.initial))).toEqual([
            'datamanagement', 'shanker_project', 'ghost_imaging', 'unrelated',
        ])
    })

    it('mtime ascending puts the oldest first', () => {
        expect(names(sortProjects(projects, {key: 'mtime', dir: 'asc'}))).toEqual([
            'unrelated', 'ghost_imaging', 'shanker_project', 'datamanagement',
        ])
    })

    it('name runs A - Z and back', () => {
        const az = ['datamanagement', 'ghost_imaging', 'shanker_project', 'unrelated']
        expect(names(sortProjects(projects, {key: 'name', dir: 'asc'}))).toEqual(az)
        expect(names(sortProjects(projects, {key: 'name', dir: 'desc'}))).toEqual([...az].reverse())
    })

    it('compares names regardless of case and digit runs as numbers', () => {
        const runs = [p('run10', 'a', [], 1), p('Run9', 'a', [], 1), p('run2', 'a', [], 1)]
        expect(names(sortProjects(runs, {key: 'name', dir: 'asc'}))).toEqual(['run2', 'Run9', 'run10'])
    })

    it('owner sorts numerically too, so user6 comes before user15 and a bare UID first', () => {
        const owned = [p('a', 'user15', [], 1), p('b', 'user6', [], 1), p('c', '90000001', [], 1)]
        expect(names(sortProjects(owned, {key: 'owner', dir: 'asc'}))).toEqual(['c', 'b', 'a'])
    })

    it('breaks ties by name A - Z, in either direction', () => {
        const tied = [p('zeta', 'bob', [], 5), p('alpha', 'bob', [], 5), p('mid', 'amy', [], 9)]
        expect(names(sortProjects(tied, {key: 'owner', dir: 'desc'}))).toEqual(['alpha', 'zeta', 'mid'])
        expect(names(sortProjects(tied, {key: 'mtime', dir: 'asc'}))).toEqual(['alpha', 'zeta', 'mid'])
    })

    it('does not mutate the input array', () => {
        const before = names(projects)
        sortProjects(projects, {key: 'name', dir: 'asc'})
        expect(names(projects)).toEqual(before)
    })
})

describe('PROJECT_SORT', () => {
    it('starts names and owners A - Z and dates newest first', () => {
        expect(nextSort(PROJECT_SORT, PROJECT_SORT.initial, 'name')).toEqual({key: 'name', dir: 'asc'})
        expect(nextSort(PROJECT_SORT, PROJECT_SORT.initial, 'owner')).toEqual({key: 'owner', dir: 'asc'})
        expect(nextSort(PROJECT_SORT, {key: 'name', dir: 'desc'}, 'mtime')).toEqual({key: 'mtime', dir: 'desc'})
    })
})

describe('canonicalProjectPath', () => {
    it('builds the project path from relpath', () => {
        expect(canonicalProjectPath('data/ghost_imaging')).toBe('/data/ghost_imaging')
    })

    it('appends the dataset segment when given', () => {
        expect(canonicalProjectPath('data/ghost_imaging', '2025-06-18')).toBe(
            '/data/ghost_imaging/2025-06-18',
        )
    })
})

describe('sharedDatasetNames', () => {
    // Shares name their project and session (#172); `dest_path` is along for
    // the ride and must not be what the match is made on.
    const share = (id: string, project: string, session: string): IngestShareV2 => ({
        resource: null,
        share_id: id,
        share_name: `share-${id}`,
        dest_path: '/data/adhoc/decoy/decoy',
        created_timestamp: 1700000000,
        username: 'ingest',
        password: 'secret',
        uid: 5001,
        gid: 5001,
        project,
        session,
        smb_host: 'ingest.example.test',
        workgroup: 'LAB',
    })
    const shares: IngestShareV2[] = [
        share('a', 'ghost_imaging', '2025-06-18'),
        share('b', 'ghost_imaging', '2025-06-19'),
        share('c', 'other_project', '2025-06-18'),
    ]

    it('matches only sessions of the given project', () => {
        const names = sharedDatasetNames(shares, 'ghost_imaging')
        expect(names).toEqual(new Set(['2025-06-18', '2025-06-19']))
    })

    it('does not match a same-named session in another project', () => {
        const names = sharedDatasetNames(shares, 'other_project')
        expect(names).toEqual(new Set(['2025-06-18']))
    })

    it('returns an empty set when nothing matches', () => {
        expect(sharedDatasetNames(shares, 'empty')).toEqual(new Set())
        expect(sharedDatasetNames([], 'ghost_imaging')).toEqual(new Set())
    })

    it('finds the share for a specific session', () => {
        expect(findDatasetShare(shares, 'ghost_imaging', '2025-06-19')?.share_id).toBe('b')
    })

    it('does not match a same-named session in another project', () => {
        expect(findDatasetShare(shares, 'ghost_imaging', '2025-06-18')?.share_id).toBe('a')
        expect(findDatasetShare(shares, 'other_project', '2025-06-19')).toBeNull()
    })

    it('returns null when the session has no share', () => {
        expect(findDatasetShare(shares, 'ghost_imaging', '2025-01-01')).toBeNull()
    })
})

describe('sortSessions', () => {
    const d = (name: string, ctime: number, mtime: number, immutable = false): SerializedDataset => ({
        name,
        ctime,
        mtime,
        immutable,
    })
    const names = (ds: SerializedDataset[]) => ds.map((x) => x.name)
    const none = new Set<string>()

    // ctime and mtime disagree on purpose, as they do on prod; only mtime may count.
    const sessions = [d('run10', 100, 200), d('Run9', 300, 100), d('run2', 200, 300, true)]

    it('starts newest-modified first without mutating the input', () => {
        expect(names(sortSessions(sessions, SESSION_SORT.initial, none))).toEqual(['run2', 'run10', 'Run9'])
        expect(names(sessions)).toEqual(['run10', 'Run9', 'run2'])
    })

    it('Last modified runs both ways on mtime, never ctime', () => {
        expect(names(sortSessions(sessions, {key: 'mtime', dir: 'asc'}, none))).toEqual(['Run9', 'run10', 'run2'])
    })

    it('compares names regardless of case and digit runs as numbers', () => {
        expect(names(sortSessions(sessions, {key: 'name', dir: 'asc'}, none))).toEqual(['run2', 'Run9', 'run10'])
        expect(names(sortSessions(sessions, {key: 'name', dir: 'desc'}, none))).toEqual(['run10', 'Run9', 'run2'])
    })

    it('Status puts a session with an ingest share first, then open, then sealed', () => {
        const mixed = [d('sealed', 1, 1, true), d('open', 1, 1), d('sharing', 1, 1)]
        const shared = new Set(['sharing'])
        expect(names(sortSessions(mixed, {key: 'status', dir: 'asc'}, shared))).toEqual(['sharing', 'open', 'sealed'])
        expect(names(sortSessions(mixed, {key: 'status', dir: 'desc'}, shared))).toEqual(['sealed', 'open', 'sharing'])
    })

    it('a sealed session with an ingest share still counts as sharing', () => {
        const mixed = [d('open', 1, 1), d('sealed-shared', 1, 1, true)]
        expect(names(sortSessions(mixed, {key: 'status', dir: 'asc'}, new Set(['sealed-shared']))))
            .toEqual(['sealed-shared', 'open'])
    })

    it('breaks ties by name A - Z, in either direction', () => {
        const tied = [d('zeta', 1, 5), d('alpha', 2, 5), d('mid', 3, 5, true)]
        expect(names(sortSessions(tied, {key: 'mtime', dir: 'desc'}, none))).toEqual(['alpha', 'mid', 'zeta'])
        expect(names(sortSessions(tied, {key: 'status', dir: 'asc'}, none))).toEqual(['alpha', 'zeta', 'mid'])
    })
})

describe('SESSION_SORT', () => {
    it('starts names A - Z, dates newest first and Status most active first', () => {
        expect(nextSort(SESSION_SORT, SESSION_SORT.initial, 'name')).toEqual({key: 'name', dir: 'asc'})
        expect(nextSort(SESSION_SORT, SESSION_SORT.initial, 'status')).toEqual({key: 'status', dir: 'asc'})
        expect(nextSort(SESSION_SORT, {key: 'name', dir: 'asc'}, 'mtime')).toEqual({key: 'mtime', dir: 'desc'})
    })
})

describe('availableMembers', () => {
    const users = ['weber', 'shanker', 'landers', 'clausen', 'bangun']

    it('excludes the owner and already-selected members', () => {
        const result = availableMembers(users, 'weber', ['shanker'], '')
        expect(result).not.toContain('weber')
        expect(result).not.toContain('shanker')
        expect(result).toContain('landers')
    })

    it('filters by a case-insensitive query and sorts alphabetically', () => {
        expect(availableMembers(users, 'weber', [], 'AN')).toEqual([
            'bangun',
            'landers',
            'shanker',
        ])
    })

    it('returns an empty list when everyone is taken', () => {
        const taken = users.slice(1)
        expect(availableMembers(users, 'weber', taken, '')).toEqual([])
    })
})

describe('memberChanges', () => {
    it('reports added and removed members against the original set', () => {
        expect(memberChanges(['landers', 'shanker'], ['shanker', 'clausen'])).toEqual({
            added: ['clausen'],
            removed: ['landers'],
        })
    })

    it('returns empty lists when the selection is unchanged (order-independent)', () => {
        expect(memberChanges(['landers', 'shanker'], ['shanker', 'landers'])).toEqual({
            added: [],
            removed: [],
        })
    })

    it('handles a previously empty membership and sorts the result', () => {
        expect(memberChanges([], ['shanker', 'bangun', 'landers'])).toEqual({
            added: ['bangun', 'landers', 'shanker'],
            removed: [],
        })
    })

    it('reports all removed when the selection is cleared', () => {
        expect(memberChanges(['shanker', 'landers'], [])).toEqual({
            added: [],
            removed: ['landers', 'shanker'],
        })
    })
})
