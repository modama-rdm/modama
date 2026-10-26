import {describe, expect, it} from 'vitest'
import {nextSort, parseSort, type SortSpec} from './sort'

const spec: SortSpec<'name' | 'date'> = {
    firstDir: {name: 'asc', date: 'desc'},
    initial: {key: 'date', dir: 'desc'},
}

describe('nextSort', () => {
    it('flips the direction when the active column is clicked again', () => {
        expect(nextSort(spec, {key: 'name', dir: 'asc'}, 'name')).toEqual({key: 'name', dir: 'desc'})
        expect(nextSort(spec, {key: 'date', dir: 'desc'}, 'date')).toEqual({key: 'date', dir: 'asc'})
    })

    it("starts another column in its own first direction, whatever the current one's was", () => {
        expect(nextSort(spec, {key: 'date', dir: 'desc'}, 'name')).toEqual({key: 'name', dir: 'asc'})
        expect(nextSort(spec, {key: 'name', dir: 'asc'}, 'date')).toEqual({key: 'date', dir: 'desc'})
    })
})

describe('parseSort', () => {
    it('reads back what was stored', () => {
        expect(parseSort(spec, '{"key":"name","dir":"desc"}')).toEqual({key: 'name', dir: 'desc'})
    })

    it.each([
        ['nothing stored', null],
        ['not JSON', 'name'],
        ['JSON null', 'null'],
        ['an unknown column', '{"key":"size","dir":"asc"}'],
        ['an inherited property as the column', '{"key":"toString","dir":"asc"}'],
        ['an unknown direction', '{"key":"name","dir":"up"}'],
    ])('falls back to the starting sort for %s', (_, raw) => {
        expect(parseSort(spec, raw)).toEqual(spec.initial)
    })
})
