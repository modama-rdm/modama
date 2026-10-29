import {describe, expect, it} from 'vitest'
import {descriptionDigest, formatBytes, sessionNamePrefix} from './format'

describe('sessionNamePrefix', () => {
    it('formats a local date as YYYY-MM-DD with a trailing dash', () => {
        // Month is 0-indexed: 5 = June.
        expect(sessionNamePrefix(new Date(2026, 5, 25))).toBe('2026-06-25-')
    })

    it('zero-pads single-digit months and days', () => {
        expect(sessionNamePrefix(new Date(2026, 0, 3))).toBe('2026-01-03-')
    })
})

describe('formatBytes', () => {
    it('keeps raw bytes below 1 KiB', () => {
        expect(formatBytes(512)).toBe('512 B')
    })

    it('scales to binary units with one decimal under 10', () => {
        expect(formatBytes(1536)).toBe('1.5 KB')
        expect(formatBytes(5 * 1024 * 1024 * 1024)).toBe('5.0 GB')
    })

    it('drops the decimal at 10 units or more', () => {
        expect(formatBytes(42 * 1024 * 1024)).toBe('42 MB')
    })
})

describe('descriptionDigest', () => {
    it('is the first line', () => {
        expect(descriptionDigest('Ni nanowires\nTitan G2, 300 kV')).toBe('Ni nanowires')
    })

    it('skips blank lines and surrounding spaces', () => {
        expect(descriptionDigest('\n  \n   Ni nanowires  \nmore')).toBe('Ni nanowires')
    })

    it('drops the Markdown that opens the line', () => {
        expect(descriptionDigest('# First light\nAlignment')).toBe('First light')
        expect(descriptionDigest('### Run 2')).toBe('Run 2')
        expect(descriptionDigest('> quoted')).toBe('quoted')
        expect(descriptionDigest('- item')).toBe('item')
        expect(descriptionDigest('* item')).toBe('item')
        expect(descriptionDigest('1. step')).toBe('step')
        expect(descriptionDigest('> - nested')).toBe('nested')
    })

    it('keeps what only looks like Markdown', () => {
        // No space after the marker: not a heading or a bullet.
        expect(descriptionDigest('#hashtag')).toBe('#hashtag')
        expect(descriptionDigest('-5 °C')).toBe('-5 °C')
        expect(descriptionDigest('2026-10-29 run')).toBe('2026-10-29 run')
    })

    it('moves past a line that is only Markdown', () => {
        expect(descriptionDigest('#\n---\ntext')).toBe('text')
        expect(descriptionDigest('* * *\n___\ntext')).toBe('text')
        expect(descriptionDigest('> \nquote ended')).toBe('quote ended')
    })

    it('is null without a description', () => {
        expect(descriptionDigest(null)).toBeNull()
        expect(descriptionDigest(undefined)).toBeNull()
        expect(descriptionDigest('')).toBeNull()
        expect(descriptionDigest(' \n\t\n')).toBeNull()
    })
})
