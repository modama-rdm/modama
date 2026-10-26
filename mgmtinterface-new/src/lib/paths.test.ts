import {describe, expect, it} from 'vitest'
import {parentOf, pathPyramid} from './paths'

describe('pathPyramid', () => {
    it('starts at the labelled root and adds one crumb per segment', () => {
        expect(pathPyramid('/')).toEqual([['/', 'er-c-data']])
        expect(pathPyramid('/adhoc/proj/2026-10-02 nested series')).toEqual([
            ['/', 'er-c-data'],
            ['/adhoc', 'adhoc'],
            ['/adhoc/proj', 'proj'],
            ['/adhoc/proj/2026-10-02 nested series', '2026-10-02 nested series'],
        ])
    })
})

describe('parentOf', () => {
    it('is the folder above, and null at the root', () => {
        expect(parentOf('/')).toBeNull()
        expect(parentOf('/adhoc')).toBe('/')
        expect(parentOf('/adhoc/proj/sess')).toBe('/adhoc/proj')
    })
})
