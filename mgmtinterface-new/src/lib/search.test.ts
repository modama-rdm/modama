import {describe, expect, it} from 'vitest'
import {matches, normalize, rankBy} from './search'

describe('normalize', () => {
    it('lowercases and makes dash, underscore, dot and whitespace one space', () => {
        expect(normalize('2026-09-30 4D-STEM map 01')).toBe('2026 09 30 4d stem map 01')
        expect(normalize('demo_underscores_2024')).toBe('demo underscores 2024')
        expect(normalize('v1.2 calibration')).toBe('v1 2 calibration')
    })

    it('collapses runs of separators and trims the ends', () => {
        expect(normalize('  demo--_.holo  ')).toBe('demo holo')
        expect(normalize('-')).toBe('')
    })
})

describe('matches', () => {
    const fields = ['demo-holography', 'shanker']

    it('matches everything on a blank query, or one that is only separators', () => {
        expect(matches(fields, '')).toBe(true)
        expect(matches(fields, '   ')).toBe(true)
        expect(matches(fields, '- _ .')).toBe(true)
    })

    it('finds a word anywhere in a field, regardless of case', () => {
        expect(matches(fields, 'HOLO')).toBe(true)
        expect(matches(fields, 'graphy')).toBe(true)
        expect(matches(fields, 'holograph')).toBe(true)
    })

    it('treats a space, dash, underscore and dot in the query alike', () => {
        expect(matches(fields, 'demo holo')).toBe(true)
        expect(matches(fields, 'demo-holo')).toBe(true)
        expect(matches(fields, 'demo_holo')).toBe(true)
        expect(matches(fields, 'demo.holo')).toBe(true)
        expect(matches(['demo_holography'], 'demo-holo')).toBe(true)
        expect(matches(['v1.2 calibration'], 'v1 2')).toBe(true)
        expect(matches(['v1_2'], 'v1.2')).toBe(true)
    })

    it('needs every word, in any order', () => {
        expect(matches(['2026-09-30 4D-STEM map 01'], 'stem map')).toBe(true)
        expect(matches(['2026-09-30 4D-STEM map 01'], 'map stem')).toBe(true)
        expect(matches(['2026-09-30 4D-STEM map 01'], 'map 2026')).toBe(true)
        expect(matches(['2026-09-30 4D-STEM map 01'], 'stem maps')).toBe(false)
        expect(matches(['2026-09-30 4D-STEM map 01'], 'stem map run')).toBe(false)
        // A word matches inside a digit run too: "02" is in "2026".
        expect(matches(['2026-09-30 4D-STEM map 01'], 'stem map 02')).toBe(true)
    })

    it('lets each word match a different field', () => {
        expect(matches(fields, 'shanker demo')).toBe(true)
        expect(matches(fields, 'shanker nope')).toBe(false)
    })

    it('does not correct a typo', () => {
        expect(matches(fields, 'holgraphy')).toBe(false)
    })

    it('does not match across a separator the name has not got', () => {
        // "demo holo" is one phrase: it does not find a name with something in between.
        expect(matches(['demo-of-holography'], 'demo-holo')).toBe(false)
        expect(matches(['demo-of-holography'], 'demo holo')).toBe(true)
    })

    it('matches nothing when there are no fields and the query is not blank', () => {
        expect(matches([], 'x')).toBe(false)
        expect(matches([], '')).toBe(true)
    })
})

describe('rankBy', () => {
    const item = (name: string, owner = 'weber') => ({name, owner})
    const fields = (i: {name: string; owner: string}) => [i.name, i.owner]
    const names = (items: {name: string}[]) => items.map((i) => i.name)
    const items = [
        item('my-demo-holo'),
        item('unrelated', 'demo'),
        item('demo-holography'),
        item('demo_holo_2024'),
        item('other'),
    ]

    it('gives the list back as it is for a blank query', () => {
        expect(rankBy(items, '', fields)).toBe(items)
        expect(rankBy(items, ' \t', fields)).toBe(items)
    })

    it('drops what does not match and keeps the order of what does', () => {
        expect(names(rankBy(items, 'holo', fields))).toEqual([
            'my-demo-holo', 'demo-holography', 'demo_holo_2024',
        ])
    })

    it('puts a name that starts with the query first, the rest in their order', () => {
        expect(names(rankBy(items, 'demo holo', fields))).toEqual([
            'demo-holography', 'demo_holo_2024', 'my-demo-holo',
        ])
        expect(names(rankBy(items, 'holo demo', fields))).toEqual([
            'my-demo-holo', 'demo-holography', 'demo_holo_2024',
        ])
    })

    it('ranks on the first field only: a match on the owner is not a prefix match', () => {
        expect(names(rankBy(items, 'demo', fields))).toEqual([
            'demo-holography', 'demo_holo_2024', 'my-demo-holo', 'unrelated',
        ])
    })

    it('compares the prefix with the separators normalized too', () => {
        expect(names(rankBy(items, 'DEMO-HOLO', fields))).toEqual([
            'demo-holography', 'demo_holo_2024', 'my-demo-holo',
        ])
    })

    it('does not mutate the input', () => {
        const before = names(items)
        rankBy(items, 'demo', fields)
        expect(names(items)).toEqual(before)
    })
})
