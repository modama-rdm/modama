import {describe, expect, it} from 'vitest'
import type {SerializedShare} from '../api/types'
import {INGEST_SMB_HOST, INGEST_WORKGROUP, shareAccess} from './shares.ts'

const share: SerializedShare = {
    resource: null,
    share_id: 'abc123',
    share_name: 'drop_abc123',
    dest_path: '/srv/smb/data/ghost_imaging/2025-06-18',
    created_timestamp: 1700000000,
    username: 'ingest7',
    password: 's3cr3t',
    uid: 5001,
    gid: 5001,
}

describe('shareAccess', () => {
    const a = shareAccess(share)

    it('builds the Windows Explorer UNC address and username', () => {
        expect(a.winAddress).toBe(`\\\\${INGEST_SMB_HOST}\\drop_abc123`)
        expect(a.winUsername).toBe(`${INGEST_WORKGROUP}\\ingest7`)
    })

    it('builds the net use map command with password and user', () => {
        expect(a.winMap).toBe(
            `net use /persistent:no z: \\\\${INGEST_SMB_HOST}\\drop_abc123 ` +
            `s3cr3t /USER:${INGEST_WORKGROUP}\\ingest7`,
        )
    })

    it('builds the unmap commands for drive and Explorer share', () => {
        expect(a.winUnmap).toBe('net use z: /delete')
        expect(a.winUnmapExplorer).toBe(
            `net use \\\\${INGEST_SMB_HOST}\\drop_abc123 /delete`,
        )
    })

    it('builds the Linux/macOS smb:// address and exposes the password', () => {
        expect(a.unixAddress).toBe(`smb://ingest7@${INGEST_SMB_HOST}/drop_abc123`)
        expect(a.password).toBe('s3cr3t')
    })

    it('prefers the host and workgroup a share from the new API names', () => {
        // The legacy list omits them, so the constants stay as the fallback.
        const c = shareAccess({...share, smb_host: 'ingest.example.test', workgroup: 'LAB'})
        expect(c.winAddress).toBe('\\\\ingest.example.test\\drop_abc123')
        expect(c.winUsername).toBe('LAB\\ingest7')
        expect(c.unixAddress).toBe('smb://ingest7@ingest.example.test/drop_abc123')
    })

    it('accepts injected host and workgroup overrides', () => {
        const b = shareAccess(share, 'example.test', 'WG')
        expect(b.winAddress).toBe('\\\\example.test\\drop_abc123')
        expect(b.winUsername).toBe('WG\\ingest7')
        expect(b.unixAddress).toBe('smb://ingest7@example.test/drop_abc123')
    })
})
