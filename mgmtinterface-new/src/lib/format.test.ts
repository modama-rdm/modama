import {describe, expect, it} from 'vitest'
import {formatBytes, sessionNamePrefix} from './format'

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
