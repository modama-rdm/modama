import {Fragment, type ReactNode} from 'react'
import {Link, useLoaderData} from 'react-router-dom'
import type {BrowseAccess, BrowseEntry, BrowseInfo} from '../api/types'
import {CopyButton, CopyField} from '../components/CopyField'
import {Card} from '../components/ui/Card'
import {
    ExternalLinkIcon,
    FileIcon,
    FolderIcon,
} from '../components/ui/icons'
import {formatBytes} from '../lib/format'
import type {BrowseLoaderData} from './browse.loader'

/** Link to another path in the data browser. */
function browsePath(path: string): string {
    return `/browse?path=${encodeURIComponent(path)}`
}

/** WebDAV reports size as a numeric string; render it human-readably, else "—". */
function formatSize(size: string | number | null): string {
    if (size == null || size === '') return '—'
    const n = typeof size === 'number' ? size : Number(size)
    return Number.isFinite(n) ? formatBytes(n) : String(size)
}

/** App-integration links, in the order the legacy browse page shows them. */
const APP_LINKS: { key: keyof BrowseAccess; label: string }[] = [
    {key: 'nextcloud', label: 'Nextcloud'},
    {key: 'jupyter', label: 'JupyterHub'},
    {key: 'libertem', label: 'LiberTEM'},
    {key: 'imageviewer', label: 'Image viewer'},
    {key: 'sampledb', label: 'SampleDB'},
]

const accessLinkClass =
    'inline-flex items-center gap-2 rounded-lg border border-default bg-surface px-4 py-2 ' +
    'text-sm font-semibold text-fg-muted transition-colors hover:bg-surface-muted'

function DetailRow({label, children}: { label: string; children: ReactNode }) {
    return (
        <div className="grid grid-cols-[8rem_1fr] gap-4 px-5 py-3.5">
            <dt className="text-[11px] font-bold uppercase tracking-wide text-fg-faint">
                {label}
            </dt>
            <dd className="text-sm text-fg">{children}</dd>
        </div>
    )
}

/** File/folder breadcrumb from the path pyramid. */
function Breadcrumb({pyramid}: { pyramid: [string, string][] }) {
    return (
        <nav className="mb-4 flex flex-wrap items-center gap-x-1 gap-y-1 text-sm text-fg-subtle">
            {pyramid.map(([full, tail], i) => (
                <Fragment key={full}>
                    {i > 0 && <span className="text-fg-faint">/</span>}
                    <Link
                        to={browsePath(full)}
                        className="rounded px-1 hover:bg-surface-muted hover:text-fg"
                    >
                        {tail}
                    </Link>
                </Fragment>
            ))}
        </nav>
    )
}

/** Compact per-row external links for the listing table. */
function RowLinks({access}: { access: BrowseAccess }) {
    return (
        <div className="flex flex-wrap items-center justify-end gap-x-3 gap-y-1">
            {APP_LINKS.filter(({key}) => access[key]).map(({key, label}) => (
                <a
                    key={key}
                    href={access[key]}
                    target="_blank"
                    rel="noreferrer"
                    className="inline-flex items-center gap-1 text-[11px] text-fg-faint hover:text-primary"
                >
                    <ExternalLinkIcon className="h-3 w-3"/>
                    {label}
                </a>
            ))}
        </div>
    )
}

const thClass =
    'px-5 py-2.5 text-left text-[11px] font-bold uppercase tracking-wide text-fg-faint'

function Listing({entries}: { entries: BrowseEntry[] }) {
    return (
        <Card className="overflow-hidden">
            <table className="w-full border-collapse">
                <thead>
                <tr className="border-b border-default">
                    <th className={thClass}>Name</th>
                    <th className={thClass}>Modified</th>
                    <th className={`${thClass} text-right`}>Size</th>
                    <th className={`${thClass} text-right`}>Open in</th>
                </tr>
                </thead>
                <tbody>
                {entries.map((e) => {
                    const isUp = e.name === '..'
                    return (
                        <tr
                            key={e.path + e.name}
                            className="border-b border-default/60 last:border-0"
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
                            <td className="px-5 py-3 text-sm text-fg-muted">
                                {e.modified ?? '—'}
                            </td>
                            <td className="px-5 py-3 text-right text-sm text-fg-muted">
                                {e.isdir ? '—' : formatSize(e.size)}
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

function BrowseBody({
                        info,
                        access,
                    }: {
    info: BrowseInfo
    access: BrowseAccess
}) {
    return (
        <>
            <Card className="mb-8">
                <dl className="divide-y divide-default/60">
                    <DetailRow label="Project">
                        {info.project ? (
                            <Link
                                to={`/projects/${encodeURIComponent(info.project)}`}
                                className="text-primary hover:underline"
                            >
                                {info.project}
                            </Link>
                        ) : (
                            <span className="text-fg-faint">—</span>
                        )}
                    </DetailRow>
                    <DetailRow label="Created">{info.created ?? '—'}</DetailRow>
                    <DetailRow label="Modified">{info.modified ?? '—'}</DetailRow>
                    {info.isdir ? (
                        <DetailRow label="Entries">
                            {info.member_count}
                            {info.truncated && (
                                <span className="ml-2 text-fg-faint">
                                    (showing first 1024)
                                </span>
                            )}
                        </DetailRow>
                    ) : (
                        <DetailRow label="Size">{formatSize(info.size)}</DetailRow>
                    )}
                </dl>
            </Card>

            <h2 className="mb-3 text-lg font-bold text-fg">Access this {info.isdir ? 'folder' : 'file'}</h2>
            <div className="mb-6 flex flex-wrap gap-3">
                {APP_LINKS.filter(({key}) => access[key]).map(({key, label}) => (
                    <a
                        key={key}
                        href={access[key]}
                        target="_blank"
                        rel="noreferrer"
                        className={accessLinkClass}
                    >
                        <ExternalLinkIcon className="h-4 w-4"/>
                        {label}
                    </a>
                ))}
            </div>

            <div className="mb-8 space-y-3">
                {access.rsync && (
                    <CopyField
                        label="Rsync"
                        value={access.rsync}
                        hint="push to er-c-data with optimized settings"
                    />
                )}
                {access.sftp && <CopyField label="SFTP" value={access.sftp}/>}
                {access.smb_windows && (
                    <CopyField label="SMB (Windows)" value={access.smb_windows}/>
                )}
                {access.smb_unix && (
                    <CopyField label="SMB (Mac, Linux)" value={access.smb_unix}/>
                )}
            </div>
        </>
    )
}

export function Browse() {
    const {result} = useLoaderData() as BrowseLoaderData

    // `errno` can't discriminate the union (the error's errno is a plain number),
    // so key the success case off the `info` field — as elsewhere in the app.
    if (!('info' in result)) {
        return (
            <div className="mx-auto max-w-270 px-6 py-8 animate-[fadeIn_0.2s_ease-out]">
                <BrowseError message={result.message} backto={result.backto}/>
            </div>
        )
    }

    const {info, access, path_pyramid, members} = result

    return (
        <div className="mx-auto max-w-270 px-6 py-8 animate-[fadeIn_0.2s_ease-out]">
            <Breadcrumb pyramid={path_pyramid}/>

            <div className="mb-6 flex items-center gap-3">
                {info.isdir ? (
                    <FolderIcon className="h-7 w-7 shrink-0 text-fg-subtle"/>
                ) : (
                    <FileIcon className="h-7 w-7 shrink-0 text-fg-subtle"/>
                )}
                <h1 className="min-w-0 truncate font-mono text-2xl font-bold text-fg">
                    {info.name}
                </h1>
                <CopyButton value={result.path} label="path"/>
            </div>

            <BrowseBody info={info} access={access}/>

            {info.isdir && members.length > 0 && (
                <>
                    <h2 className="mb-3 text-lg font-bold text-fg">Browse</h2>
                    <Listing entries={members}/>
                </>
            )}
        </div>
    )
}
