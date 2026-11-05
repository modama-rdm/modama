import {describe, expect, it} from 'vitest'
import source from '../../CHANGELOG.md?raw'
import {CHANGELOG, parseChangelog} from './changelog'

describe('parseChangelog', () => {
    it('reads version, date and title from a release heading', () => {
        const [entry] = parseChangelog('## [1.2.3] - 2026-11-05 - A title\n\n### Fixed\n\n- One fix.\n')
        expect(entry).toEqual({version: '1.2.3', date: '2026-11-05', title: 'A title', changes: ['One fix.']})
    })

    it('allows a release without a date', () => {
        const [entry] = parseChangelog('## [0.9.0] - Ingest share\n\n### Added\n\n- A share.\n')
        expect(entry.date).toBeNull()
        expect(entry.title).toBe('Ingest share')
    })

    it('joins a bullet continued on indented lines', () => {
        const [entry] = parseChangelog('## [1.0.0] - 2026-01-01 - T\n\n### Added\n\n- First line\n  second line.\n- Next.\n')
        expect(entry.changes).toEqual(['First line second line.', 'Next.'])
    })

    it('lists the bullets of every section in file order and leaves out Internal', () => {
        const [entry] = parseChangelog([
            '## [1.0.0] - 2026-01-01 - T',
            '### Added',
            '- Added a thing.',
            '### Internal',
            '- A note for developers.',
            '### Fixed',
            '- Fixed a thing.',
        ].join('\n'))
        expect(entry.changes).toEqual(['Added a thing.', 'Fixed a thing.'])
    })

    it('drops issue references in parentheses', () => {
        const [entry] = parseChangelog('## [1.0.0] - 2026-01-01 - T\n### Added\n- A thing. (#1)\n- Another (#22) thing.\n')
        expect(entry.changes).toEqual(['A thing.', 'Another thing.'])
    })

    it('skips an Unreleased part and the text above the first release', () => {
        const entries = parseChangelog([
            '# Changelog',
            '- not a release',
            '## [Unreleased]',
            '### Added',
            '- Not shipped yet.',
            '## [1.0.0] - 2026-01-01 - T',
            '### Added',
            '- Shipped.',
        ].join('\n'))
        expect(entries).toEqual([{version: '1.0.0', date: '2026-01-01', title: 'T', changes: ['Shipped.']}])
    })
})

describe('CHANGELOG.md', () => {
    const versions = CHANGELOG.map((entry) => entry.version)

    it('starts with the version in package.json, which the footer shows', () => {
        expect(versions[0]).toBe(__APP_VERSION__)
    })

    it('parses every release heading', () => {
        const headings = source.split('\n').filter((line) => line.startsWith('## [') && !line.startsWith('## [Unreleased]'))
        expect(CHANGELOG).toHaveLength(headings.length)
    })

    it('gives every release a title and something to show', () => {
        for (const entry of CHANGELOG) {
            expect(entry.title, entry.version).not.toBe('')
            expect(entry.changes.length, entry.version).toBeGreaterThan(0)
        }
    })

    it('lists releases newest first, each version once', () => {
        const byNumber = (v: string) => v.split('.').map(Number)
        const sorted = [...versions].sort((a, b) => {
            const [x, y] = [byNumber(a), byNumber(b)]
            return y[0] - x[0] || y[1] - x[1] || y[2] - x[2]
        })
        expect(versions).toEqual(sorted)
        expect(new Set(versions).size).toBe(versions.length)
    })
})
