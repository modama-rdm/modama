import {describe, expect, it} from 'vitest'
import type {
    ListSharesResponse,
    SerializedDataset,
    SerializedProject,
    SerializedShare,
} from '../api/types'
import {
    availableMembers,
    canonicalProjectPath,
    filterProjects,
    groupProjects,
    parseSharePath,
    sharedDatasetNames,
    sortDatasets,
    sortProjects,
} from './projectsUtil.ts'

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
    it('returns all projects for an empty query', () => {
        expect(filterProjects(projects, '')).toHaveLength(projects.length)
    })

    it('matches names case-insensitively', () => {
        const result = filterProjects(projects, 'GHOST')
        expect(result.length).toBeGreaterThan(0)
        expect(result.every((p) => p.name.toLowerCase().includes('ghost'))).toBe(true)
        expect(result.some((p) => p.name === 'ghost_imaging')).toBe(true)
    })
})

describe('sortProjects', () => {
    it('"recent" orders by newest mtime first', () => {
        const sorted = sortProjects(projects, 'recent')
        for (let i = 1; i < sorted.length; i++) {
            expect(sorted[i - 1].mtime).toBeGreaterThanOrEqual(sorted[i].mtime)
        }
    })

    it('"name" orders alphabetically', () => {
        const sorted = sortProjects(projects, 'name')
        for (let i = 1; i < sorted.length; i++) {
            expect(sorted[i - 1].name.localeCompare(sorted[i].name)).toBeLessThanOrEqual(0)
        }
    })

    it('does not mutate the input array', () => {
        const before = projects.map((p) => p.name)
        sortProjects(projects, 'name')
        expect(projects.map((p) => p.name)).toEqual(before)
    })
})

describe('parseSharePath', () => {
    it('extracts project and dataset from the last two path segments', () => {
        expect(parseSharePath('/data/adhoc/ghost_imaging/2025-06-18')).toEqual({
            project: 'ghost_imaging',
            dataset: '2025-06-18',
        })
    })

    it('ignores trailing slashes', () => {
        expect(parseSharePath('/data/adhoc/proj/sess/')).toEqual({
            project: 'proj',
            dataset: 'sess',
        })
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
    const share = (destPath: string): SerializedShare => ({
        resource: null,
        share_id: destPath,
        share_name: 'tmp',
        dest_path: destPath,
        created_timestamp: 1700000000,
        username: 'ingest',
        password: 'secret',
        uid: 5001,
        gid: 5001,
    })
    const shares: ListSharesResponse = {
        a: share('/srv/smb/data/ghost_imaging/2025-06-18'),
        b: share('/srv/smb/data/ghost_imaging/2025-06-19/'),
        c: share('/srv/smb/data/other_project/2025-06-18'),
    }

    it('matches only datasets under the given project relpath', () => {
        const names = sharedDatasetNames(shares, 'data/ghost_imaging')
        expect(names).toEqual(new Set(['2025-06-18', '2025-06-19']))
    })

    it('does not match a same-named dataset in another project', () => {
        const names = sharedDatasetNames(shares, 'data/other_project')
        expect(names).toEqual(new Set(['2025-06-18']))
    })

    it('returns an empty set when nothing matches', () => {
        expect(sharedDatasetNames(shares, 'data/empty')).toEqual(new Set())
    })
})

describe('sortDatasets', () => {
    const d = (name: string, mtime: number): SerializedDataset => ({
        name,
        mtime,
        ctime: mtime,
        immutable: false,
    })

    it('orders newest-modified first without mutating the input', () => {
        const input = [d('a', 100), d('b', 300), d('c', 200)]
        const sorted = sortDatasets(input)
        expect(sorted.map((x) => x.name)).toEqual(['b', 'c', 'a'])
        expect(input.map((x) => x.name)).toEqual(['a', 'b', 'c'])
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
