import {describe, expect, it} from 'vitest'
import type {IngestShareV2} from '../api/types'
import {shareAccess} from './shares.ts'

const share: IngestShareV2 = {
    resource: null,
    share_id: 'abc123',
    share_name: 'drop_abc123',
    dest_path: '/data/adhoc/ghost_imaging/2025-06-18',
    created_timestamp: 1700000000,
    username: 'ingest7',
    password: 's3cr3t',
    uid: 5001,
    gid: 5001,
    project: 'ghost_imaging',
    session: '2025-06-18',
    smb_host: 'ingest.example.test',
    workgroup: 'LAB',
}

describe('shareAccess', () => {
    const a = shareAccess(share)

    it('builds the Windows Explorer UNC address and username from what the share names', () => {
        // The host and workgroup come with every share from the API; no
        // constant in the client.
        expect(a.winAddress).toBe('\\\\ingest.example.test\\drop_abc123')
        expect(a.winUsername).toBe('LAB\\ingest7')
    })

    it('builds the net use map command with password and user', () => {
        expect(a.winMap).toBe(
            'net use /persistent:no z: \\\\ingest.example.test\\drop_abc123 s3cr3t /USER:LAB\\ingest7',
        )
    })

    it('builds the unmap commands for drive and Explorer share', () => {
        expect(a.winUnmap).toBe('net use z: /delete')
        expect(a.winUnmapExplorer).toBe('net use \\\\ingest.example.test\\drop_abc123 /delete')
    })

    it('builds the Linux/macOS smb:// address and exposes the password', () => {
        expect(a.unixAddress).toBe('smb://ingest7@ingest.example.test/drop_abc123')
        expect(a.password).toBe('s3cr3t')
    })

    it('accepts injected host and workgroup overrides', () => {
        const b = shareAccess(share, 'example.test', 'WG')
        expect(b.winAddress).toBe('\\\\example.test\\drop_abc123')
        expect(b.winUsername).toBe('WG\\ingest7')
        expect(b.unixAddress).toBe('smb://ingest7@example.test/drop_abc123')
    })
})
