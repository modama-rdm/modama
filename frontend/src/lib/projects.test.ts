import {describe, expect, it} from 'vitest'
import type {SerializedProject} from '../api/types'
import {
    filterProjects,
    groupProjects,
    parseSharePath,
    sortProjects,
} from './projects'

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

// Minimal fixture:
//   datamanagement  — owned by weber, shanker is a member
//   ghost_imaging   — owned by clausen, no weber/shanker
//   shanker_project — owned by shanker
//   unrelated       — owned by bryan, no weber/shanker
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
