/**
 * User avatar with deterministic per-user colour.
 *
 * These are categorical identity colours (like chart series colours), not theme
 * surface tokens — they stay constant across light/dark by design, so a fixed
 * palette is appropriate here. The username hash picks a stable entry.
 */

const PALETTE = [
    '#2563eb',
    '#7c3aed',
    '#059669',
    '#d97706',
    '#dc2626',
    '#0891b2',
    '#be185d',
    '#65a30d',
]

function colorFor(username: string): string {
    let hash = 0
    for (let i = 0; i < username.length; i++) {
        hash = (hash * 31 + username.charCodeAt(i)) | 0
    }
    return PALETTE[Math.abs(hash) % PALETTE.length]
}

function initials(username: string): string {
    return username.slice(0, 2).toUpperCase()
}

interface AvatarProps {
    username: string
    size?: number
    title?: string
    className?: string
}

export function Avatar({username, size = 32, title, className = ''}: AvatarProps) {
    return (
        <div
            title={title ?? username}
            className={`inline-flex shrink-0 items-center justify-center rounded-full font-bold text-white ${className}`}
            style={{
                width: size,
                height: size,
                background: colorFor(username),
                fontSize: Math.round(size * 0.4),
            }}
        >
            {initials(username)}
        </div>
    )
}
