import type {SVGProps} from 'react'

/**
 * Minimal inline icon set (Lucide-style line icons). They use `currentColor`,
 * so color comes from the surrounding `text-*` token and size from the
 * `h-` / `w-` utilities — professional and theme-aware without an icon dep.
 */
const base: SVGProps<SVGSVGElement> = {
    width: 24,
    height: 24,
    viewBox: '0 0 24 24',
    fill: 'none',
    stroke: 'currentColor',
    strokeWidth: 2,
    strokeLinecap: 'round',
    strokeLinejoin: 'round',
    'aria-hidden': true,
}

export function CheckCircleIcon(props: SVGProps<SVGSVGElement>) {
    return (
        <svg {...base} {...props}>
            <circle cx="12" cy="12" r="10"/>
            <path d="m9 12 2 2 4-4"/>
        </svg>
    )
}

export function XCircleIcon(props: SVGProps<SVGSVGElement>) {
    return (
        <svg {...base} {...props}>
            <circle cx="12" cy="12" r="10"/>
            <path d="m15 9-6 6"/>
            <path d="m9 9 6 6"/>
        </svg>
    )
}

export function AlertTriangleIcon(props: SVGProps<SVGSVGElement>) {
    return (
        <svg {...base} {...props}>
            <path d="m21.73 18-8-14a2 2 0 0 0-3.48 0l-8 14A2 2 0 0 0 4 21h16a2 2 0 0 0 1.73-3Z"/>
            <path d="M12 9v4"/>
            <path d="M12 17h.01"/>
        </svg>
    )
}

export function FolderIcon(props: SVGProps<SVGSVGElement>) {
    return (
        <svg {...base} {...props}>
            <path
                d="M20 20a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.9a2 2 0 0 1-1.69-.9L9.6 3.9A2 2 0 0 0 7.93 3H4a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2Z"/>
        </svg>
    )
}

export function FileIcon(props: SVGProps<SVGSVGElement>) {
    return (
        <svg {...base} {...props}>
            <path d="M15 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V7Z"/>
            <path d="M14 2v5h5"/>
        </svg>
    )
}

export function ExternalLinkIcon(props: SVGProps<SVGSVGElement>) {
    return (
        <svg {...base} {...props}>
            <path d="M15 3h6v6"/>
            <path d="M10 14 21 3"/>
            <path d="M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6"/>
        </svg>
    )
}

export function SearchIcon(props: SVGProps<SVGSVGElement>) {
    return (
        <svg {...base} {...props}>
            <circle cx="11" cy="11" r="8"/>
            <path d="m21 21-4.3-4.3"/>
        </svg>
    )
}

export function PlusIcon(props: SVGProps<SVGSVGElement>) {
    return (
        <svg {...base} {...props}>
            <path d="M5 12h14"/>
            <path d="M12 5v14"/>
        </svg>
    )
}

export function UploadIcon(props: SVGProps<SVGSVGElement>) {
    return (
        <svg {...base} {...props}>
            <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/>
            <path d="M17 8l-5-5-5 5"/>
            <path d="M12 3v12"/>
        </svg>
    )
}

export function XIcon(props: SVGProps<SVGSVGElement>) {
    return (
        <svg {...base} {...props}>
            <path d="M18 6 6 18"/>
            <path d="m6 6 12 12"/>
        </svg>
    )
}

export function ArrowLeftIcon(props: SVGProps<SVGSVGElement>) {
    return (
        <svg {...base} {...props}>
            <path d="M19 12H5"/>
            <path d="m12 19-7-7 7-7"/>
        </svg>
    )
}

export function CopyIcon(props: SVGProps<SVGSVGElement>) {
    return (
        <svg {...base} {...props}>
            <rect width="14" height="14" x="8" y="8" rx="2" ry="2"/>
            <path d="M4 16c-1.1 0-2-.9-2-2V4c0-1.1.9-2 2-2h10c1.1 0 2 .9 2 2"/>
        </svg>
    )
}

export function LockIcon(props: SVGProps<SVGSVGElement>) {
    return (
        <svg {...base} {...props}>
            <rect width="18" height="11" x="3" y="11" rx="2" ry="2"/>
            <path d="M7 11V7a5 5 0 0 1 10 0v4"/>
        </svg>
    )
}

export function UsersIcon(props: SVGProps<SVGSVGElement>) {
    return (
        <svg {...base} {...props}>
            <path d="M16 21v-2a4 4 0 0 0-4-4H6a4 4 0 0 0-4 4v2"/>
            <circle cx="9" cy="7" r="4"/>
            <path d="M22 21v-2a4 4 0 0 0-3-3.87"/>
            <path d="M16 3.13a4 4 0 0 1 0 7.75"/>
        </svg>
    )
}
