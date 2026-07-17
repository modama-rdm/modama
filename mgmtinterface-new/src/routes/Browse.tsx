import {Fragment} from 'react'
import {Link, useLoaderData} from 'react-router-dom'
import type {BrowseAccess, BrowseEntry, BrowseInfo} from '../api/types'
import {CopyButton, CopyField} from '../components/CopyField'
import {useCopy} from '../hooks/useCopy'
import {Card} from '../components/ui/Card'
import {
    ArrowLeftIcon,
    CheckCircleIcon,
    FileIcon,
    FolderIcon,
} from '../components/ui/icons'
import {formatBytes} from '../lib/format'
import {browsePath, browseUrl} from '../lib/quotePath'
import type {BrowseLoaderData} from './browse.loader'

/** WebDAV reports size as a numeric string; render it human-readably, else "—". */
function formatSize(size: string | number | null): string {
    if (size == null || size === '') return '—'
    const n = typeof size === 'number' ? size : Number(size)
    return Number.isFinite(n) ? formatBytes(n) : String(size)
}

/**
 * App-integration links, rendered as colored "Open in" badges (per the Claude
 * Design browse screen). Each carries its own palette: a solid pastel chip in
 * light mode, a translucent tint in dark mode. The color is what makes these
 * read as actionable — the previous faint-gray text was too low-contrast. Order
 * follows the legacy browse page; the two ER-C-3 links extend the palette.
 */
const APP_LINKS: {
    key: keyof BrowseAccess
    label: string
    /** Tailwind badge palette (light pastel + dark translucent + hover). */
    badge: string
}[] = [
    {
        key: 'nextcloud',
        label: 'Nextcloud',
        badge:
            'bg-sky-50 text-sky-700 hover:bg-sky-100 ' +
            'dark:bg-sky-500/15 dark:text-sky-300 dark:hover:bg-sky-500/25',
    },
    {
        key: 'jupyter',
        label: 'JupyterHub',
        badge:
            'bg-orange-50 text-orange-700 hover:bg-orange-100 ' +
            'dark:bg-orange-500/15 dark:text-orange-300 dark:hover:bg-orange-500/25',
    },
    {
        key: 'libertem',
        label: 'LiberTEM',
        badge:
            'bg-green-50 text-green-800 hover:bg-green-100 ' +
            'dark:bg-green-500/15 dark:text-green-300 dark:hover:bg-green-500/25',
    },
    {
        key: 'imageviewer',
        label: 'Image viewer',
        badge:
            'bg-violet-50 text-violet-700 hover:bg-violet-100 ' +
            'dark:bg-violet-500/15 dark:text-violet-300 dark:hover:bg-violet-500/25',
    },
    {
        key: 'sampledb',
        label: 'SampleDB',
        badge:
            'bg-fuchsia-50 text-fuchsia-800 hover:bg-fuchsia-100 ' +
            'dark:bg-fuchsia-500/15 dark:text-fuchsia-300 dark:hover:bg-fuchsia-500/25',
    },
    {
        key: 'sampledb_erc3',
        label: 'SampleDB (ER-C-3)',
        badge:
            'bg-indigo-50 text-indigo-700 hover:bg-indigo-100 ' +
            'dark:bg-indigo-500/15 dark:text-indigo-300 dark:hover:bg-indigo-500/25',
    },
    {
        key: 'elabftw_erc3',
        label: 'ElabFTW (ER-C-3)',
        badge:
            'bg-rose-50 text-rose-700 hover:bg-rose-100 ' +
            'dark:bg-rose-500/15 dark:text-rose-300 dark:hover:bg-rose-500/25',
    },
]

/**
 * A single "open in external app" badge. Renders a real external link (opens in
 * a new tab) styled as the design's colored pill. `size` matches the two places
 * it appears: `sm` for the compact listing-table rows, `md` for the top card.
 */
function AccessBadge({
                         href,
                         label,
                         badge,
                         size,
                     }: {
    href: string
    label: string
    badge: string
    size: 'sm' | 'md'
}) {
    const dims = size === 'sm' ? 'px-2.5 py-1 text-xs' : 'px-3 py-1.5 text-sm'
    return (
        <a
            href={href}
            target="_blank"
            rel="noreferrer"
            className={`inline-flex items-center rounded-md font-semibold transition-colors ${dims} ${badge}`}
        >
            {label}
        </a>
    )
}

/** Text button that copies the shareable URL for `path`, briefly flipping to
 *  a check — for referencing this location in ELN entries (#202). */
function CopyUrlButton({path}: { path: string }) {
    const [copied, copy] = useCopy()
    return (
        <button
            type="button"
            onClick={() => copy(browseUrl(path))}
            className="inline-flex shrink-0 items-center gap-1 rounded-md border border-default px-2.5 py-1 text-[11px] font-semibold text-fg-subtle transition-colors hover:bg-primary/5 hover:text-primary"
        >
            {copied ? (
                <>
                    <CheckCircleIcon className="h-3 w-3 text-success"/>
                    Copied
                </>
            ) : (
                'Copy URL'
            )}
        </button>
    )
}

/** Compact header bar for the current resource: icon + name + a meta line on the
 *  left, the full path + copy button on the right. Replaces the old detail card,
 *  matching the design's browse header. */
function ResourceBar({
                         info,
                         access,
                         path,
                     }: {
    info: BrowseInfo
    access: BrowseAccess
    path: string
}) {
    const meta = info.isdir
        ? `${info.member_count} item${info.member_count === 1 ? '' : 's'}${
            info.truncated ? ' · showing first 1024' : ''
        }`
        : formatSize(info.size)

    const FOLDER_TOP_KEYS: (keyof BrowseAccess)[] = ['nextcloud', 'jupyter']
    const links = info.isdir
        ? APP_LINKS.filter(({key}) => FOLDER_TOP_KEYS.includes(key) && access[key])
        : APP_LINKS.filter(({key}) => access[key])
    return (
        <Card className="mb-3.5 px-5 py-3.5">
            <div className="flex flex-wrap items-center justify-between gap-x-4 gap-y-3">
                <div className="flex min-w-0 items-center gap-2.5">
                    {info.isdir ? (
                        <FolderIcon className="h-5 w-5 shrink-0 text-fg-subtle"/>
                    ) : (
                        <FileIcon className="h-5 w-5 shrink-0 text-fg-subtle"/>
                    )}
                    <div className="min-w-0">
                        <div className="truncate font-mono text-sm font-semibold text-fg">
                            {info.name || '/'}
                        </div>
                        <div className="mt-0.5 text-xs text-fg-faint">
                            {info.project && (
                                <>
                                    in{' '}
                                    <Link
                                        to={`/projects/${encodeURIComponent(info.project)}`}
                                        className="text-fg-subtle hover:text-primary hover:underline"
                                    >
                                        {info.project}
                                    </Link>
                                    {' · '}
                                </>
                            )}
                            {meta}
                        </div>
                    </div>
                </div>
                <div className="flex min-w-0 items-center gap-2">
                    <code
                        title={path}
                        className="min-w-0 truncate font-mono text-[11px] text-fg-subtle"
                    >
                        {path}
                    </code>
                    <CopyUrlButton path={path}/>
                </div>
            </div>
            {links.length > 0 && (
                <div className="mt-3 flex flex-wrap items-center gap-1.5 border-t border-default/60 pt-3">
                    {links.map(({key, label, badge}) => (
                        <AccessBadge
                            key={key}
                            href={access[key]!}
                            label={label}
                            badge={badge}
                            size="sm"
                        />
                    ))}
                </div>
            )}
        </Card>
    )
}

/** The "Access this location" card: SMB / rsync / sftp connection strings as
 *  copy fields. Renders only the commands the server returned. */
function AccessCommands({access}: { access: BrowseAccess }) {
    const fields: { label: string; value: string; hint?: string }[] = []
    if (access.smb_windows)
        fields.push({label: 'SMB (Windows)', value: access.smb_windows})
    if (access.smb_unix)
        fields.push({label: 'SMB (Linux / macOS)', value: access.smb_unix})
    if (access.rsync)
        fields.push({
            label: 'rsync',
            value: access.rsync,
            hint: 'push to er-c-data with optimized settings',
        })
    if (access.sftp) fields.push({label: 'SFTP', value: access.sftp})
    if (fields.length === 0) return null
    return (
        <Card className="px-5 py-5">
            <div className="mb-3.5 text-[13px] font-bold text-fg-muted">
                Access this location
            </div>
            <div className="space-y-3">
                {fields.map((f) => (
                    <CopyField key={f.label} label={f.label} value={f.value} hint={f.hint}/>
                ))}
            </div>
        </Card>
    )
}

/** File/folder breadcrumb from the path pyramid, prefixed with a "Projects"
 *  crumb back to the app. The trailing crumb is the current
 *  location — shown as mono text, not a link. */
function Breadcrumb({pyramid}: { pyramid: [string, string][] }) {
    return (
        <nav className="mb-3.5 flex flex-wrap items-center gap-x-1.5 gap-y-1 text-[13px]">
            <Link to="/projects" className="rounded px-0.5 text-primary hover:underline">
                Projects
            </Link>
            {pyramid.map(([full, tail], i) => {
                const isLast = i === pyramid.length - 1
                return (
                    <Fragment key={full}>
                        <span className="text-fg-faint">›</span>
                        {isLast ? (
                            <span className="px-0.5 font-mono text-xs font-medium text-fg-muted">
                                {tail}
                            </span>
                        ) : (
                            <Link
                                to={browsePath(full)}
                                className="rounded px-0.5 text-primary hover:underline"
                            >
                                {tail}
                            </Link>
                        )}
                    </Fragment>
                )
            })}
        </nav>
    )
}

/** Compact per-row external links for the listing table, as colored badges. */
function RowLinks({access}: { access: BrowseAccess }) {
    return (
        <div className="flex flex-wrap items-center gap-1.5">
            {APP_LINKS.filter(({key}) => access[key]).map(({key, label, badge}) => (
                <AccessBadge
                    key={key}
                    href={access[key]!}
                    label={label}
                    badge={badge}
                    size="sm"
                />
            ))}
        </div>
    )
}

const thClass =
    'px-5 py-2.5 text-left text-[11px] font-bold uppercase tracking-wide text-fg-faint'

function Listing({entries}: { entries: BrowseEntry[] }) {
    return (
        <Card className="mb-3.5 overflow-hidden">
            <table className="w-full border-collapse">
                <thead>
                <tr className="border-b border-default">
                    <th className={thClass}>Name</th>
                    <th className={`${thClass} text-right`}>Size</th>
                    <th className={thClass}>Modified</th>
                    <th className={thClass}>Open in</th>
                </tr>
                </thead>
                <tbody>
                {entries.map((e) => {
                    const isUp = e.name === '..'
                    return (
                        <tr
                            key={e.path + e.name}
                            className="border-b border-default/60 transition-colors last:border-0 hover:bg-surface-muted/60"
                        >
                            <td className="px-5 py-3">
                                <div className="flex items-center gap-2">
                                    {e.isdir ? (
                                        <FolderIcon className="h-4 w-4 shrink-0 text-fg-faint"/>
                                    ) : (
                                        <FileIcon className="h-4 w-4 shrink-0 text-fg-faint"/>
                                    )}
                                    <Link
                                        to={browsePath(e.path)}
                                        className="truncate font-mono text-sm text-fg hover:text-primary"
                                    >
                                        {e.name}
                                    </Link>
                                    {!isUp && (
                                        <CopyButton value={e.path} label="path"/>
                                    )}
                                </div>
                            </td>
                            <td className="px-5 py-3 text-right font-mono text-xs text-fg-subtle">
                                {e.isdir ? '—' : formatSize(e.size)}
                            </td>
                            <td className="px-5 py-3 text-[13px] text-fg-muted">
                                {e.modified ?? '—'}
                            </td>
                            <td className="px-5 py-3">
                                <RowLinks access={e.access}/>
                            </td>
                        </tr>
                    )
                })}
                </tbody>
            </table>
        </Card>
    )
}

/** errno != 0 — path not found or WebDAV error. */
function BrowseError({
                         message,
                         backto,
                     }: {
    message: string
    backto: string | null
}) {
    return (
        <Card className="border-danger/40 bg-danger/5 p-8 text-center">
            <div className="text-base font-semibold text-fg">
                Could not open this path
            </div>
            <div className="mt-1.5 text-sm text-fg-muted">{message}</div>
            {backto && (
                <Link
                    to={browsePath(backto)}
                    className="mt-4 inline-flex items-center gap-1.5 text-sm font-semibold text-primary hover:underline"
                >
                    Go to {backto}
                </Link>
            )}
        </Card>
    )
}

function BackToProjects() {
    return (
        <Link
            to="/projects"
            className="mb-5 inline-flex items-center gap-1.5 text-sm text-fg-subtle hover:text-fg"
        >
            <ArrowLeftIcon className="h-4 w-4"/>
            Projects
        </Link>
    )
}

export function Browse() {
    const {result} = useLoaderData() as BrowseLoaderData

    // `errno` can't discriminate the union (the error's errno is a plain number),
    // so key the success case off the `info` field — as elsewhere in the app.
    if (!('info' in result)) {
        return (
            <div className="mx-auto max-w-270 px-6 py-8 animate-[fadeIn_0.2s_ease-out]">
                <BackToProjects/>
                <BrowseError message={result.message} backto={result.backto}/>
            </div>
        )
    }

    const {info, access, path_pyramid, members} = result

    return (
        <div className="mx-auto max-w-270 px-6 py-8 animate-[fadeIn_0.2s_ease-out]">
            <Breadcrumb pyramid={path_pyramid}/>
            <ResourceBar info={info} access={access} path={result.path}/>

            {info.isdir && members.length > 0 && <Listing entries={members}/>}

            <AccessCommands access={access}/>
        </div>
    )
}
