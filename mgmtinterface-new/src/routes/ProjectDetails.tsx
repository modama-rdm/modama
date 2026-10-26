import {Suspense, type ReactNode} from 'react'
import {
    Await,
    Link,
    Navigate,
    useAsyncError,
    useAsyncValue,
    useLoaderData,
    useNavigate,
} from 'react-router-dom'
import {Unauthorized} from '../api/errors'
import type {
    ProjectDetailsV2,
    ProjectUsageV2,
    SerializedDataset,
    SerializedProject,
} from '../api/types'
import {Breadcrumbs} from '../components/Breadcrumbs'
import {CopyButton} from '../components/CopyField'
import {Avatar} from '../components/ui/Avatar'
import {Button} from '../components/ui/Button'
import {Card} from '../components/ui/Card'
import {
    CheckCircleIcon,
    FolderIcon,
    PlusIcon,
    UsersIcon,
} from '../components/ui/icons'
import {SortHeader} from '../components/ui/SortHeader'
import {useStoredSort} from '../hooks/useStoredSort'
import {formatBytes, formatDateTime} from '../lib/format'
import {
    canonicalProjectPath,
    SESSION_SORT,
    sharedDatasetNames,
    sortSessions,
} from '../lib/projectsUtil'
import {browsePath, browseUrl} from '../lib/quotePath'
import type {ProjectDetailsLoaderData} from './projectDetails.loader'

const badgeBase =
    'inline-flex items-center gap-1 rounded px-2 py-0.5 text-[11px] font-bold uppercase tracking-wide'

function StatusBadges({dataset, shared}: { dataset: SerializedDataset; shared: boolean }) {
    return (
        <div className="flex flex-wrap items-center gap-1.5">
            {dataset.immutable ? (
                <span className={`${badgeBase} bg-success/10 text-success`}>
                    <CheckCircleIcon className="h-3 w-3"/>
                    Sealed
                </span>
            ) : (
                <span className={`${badgeBase} bg-warning/15 text-warning`}>Open</span>
            )}
            {shared && (
                <span
                    className={`${badgeBase} bg-violet-50 text-violet-700 dark:bg-violet-500/15 dark:text-violet-300`}>
                    <span className="h-1.5 w-1.5 rounded-full bg-current"/>
                    Sharing
                </span>
            )}
        </div>
    )
}

const thClass =
    'px-5 py-2.5 text-left text-[11px] font-bold uppercase tracking-wide text-fg-faint'

function SessionsTable({
                           project,
                           datasets,
                           sharedNames,
                           canMutate,
                       }: {
    project: SerializedProject
    datasets: SerializedDataset[]
    sharedNames: Set<string>
    canMutate: boolean
}) {
    const navigate = useNavigate()
    // One remembered sort for every project's sessions table (#75).
    const [sort, sortBy] = useStoredSort('erc-sessions-sort', SESSION_SORT)
    if (datasets.length === 0) {
        return (
            <Card className="p-12 text-center">
                <FolderIcon className="mx-auto h-8 w-8 text-fg-faint"/>
                <div className="mt-3 text-[15px] font-semibold text-fg">
                    No sessions yet
                </div>
                <div className="mt-1.5 text-[13px] text-fg-subtle">
                    Create a session to start organizing your measurement data.
                </div>
                {canMutate && (
                    <Button
                        onClick={() => navigate(`/projects/${project.name}/sessions/new`)}
                        className="mt-5"
                    >
                        Create first session
                    </Button>
                )}
            </Card>
        )
    }
    return (
        <Card className="overflow-hidden">
            <table className="w-full border-collapse">
                <thead>
                <tr className="border-b border-default">
                    <SortHeader label="Session" column="name" sort={sort} onSort={sortBy} className={thClass}/>
                    <SortHeader label="Last modified" column="mtime" sort={sort} onSort={sortBy} className={thClass}/>
                    <SortHeader label="Status" column="status" sort={sort} onSort={sortBy} className={thClass}/>
                    <th className={thClass}/>
                </tr>
                </thead>
                <tbody>
                {sortSessions(datasets, sort, sharedNames).map((d) => {
                    const shared = sharedNames.has(d.name)
                    return (
                        <tr
                            key={d.name}
                            className="border-b border-default/60 last:border-0"
                        >
                            <td className="px-5 py-3.5">
                                <div className="font-mono text-sm font-semibold text-fg">
                                    {d.name}
                                </div>
                            </td>
                            <td className="px-5 py-3.5 text-sm text-fg-muted">
                                {formatDateTime(d.mtime)}
                            </td>
                            <td className="px-5 py-3.5">
                                <StatusBadges dataset={d} shared={shared}/>
                            </td>
                            <td className="px-5 py-3.5">
                                <div className="flex items-center justify-end gap-2">
                                    <Button
                                        variant="secondary"
                                        onClick={() =>
                                            navigate(
                                                browsePath(
                                                    canonicalProjectPath(
                                                        project.relpath,
                                                        d.name,
                                                    ),
                                                ),
                                            )
                                        }
                                        className="px-3 py-1.5 text-xs"
                                    >
                                        Browse
                                    </Button>
                                    {canMutate && (!d.immutable || shared) && (
                                        <Button
                                            variant="secondary"
                                            onClick={() =>
                                                navigate(
                                                    `/projects/${project.name}/sessions/${d.name}/ingest`,
                                                )
                                            }
                                            className="px-3 py-1.5 text-xs"
                                        >
                                            {shared ? 'Manage ingest' : 'Start ingest'}
                                        </Button>
                                    )}
                                    {canMutate && !d.immutable && (
                                        <Button
                                            variant="dangerOutline"
                                            onClick={() =>
                                                navigate(
                                                    `/projects/${project.name}/sessions/${d.name}/seal`,
                                                )
                                            }
                                            className="px-3 py-1.5 text-xs"
                                        >
                                            Seal…
                                        </Button>
                                    )}
                                </div>
                            </td>
                        </tr>
                    )
                })}
                </tbody>
            </table>
        </Card>
    )
}

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

/**
 * The Size row's value: streams in from the separate usage call (#172), which
 * runs `du` on the server and can take a minute on a large project. Until it
 * resolves there is a placeholder; if it fails, a dash — the rest of the page
 * is not affected either way.
 */
function Size() {
    const {usage_bytes} = useAsyncValue() as ProjectUsageV2
    return <span title={`${usage_bytes} bytes`}>{formatBytes(usage_bytes)}</span>
}

function SizePending() {
    return (
        <span
            aria-busy="true"
            aria-label="Computing size"
            className="inline-block h-4 w-20 animate-pulse rounded bg-surface-muted align-middle"
        />
    )
}

/** Renders once the `details` promise resolves. */
function ProjectBody() {
    const {currentUser, shares, usage} = useLoaderData() as ProjectDetailsLoaderData
    const project = useAsyncValue() as ProjectDetailsV2
    const navigate = useNavigate()

    const isOwner = currentUser === project.owner
    const canMutate = isOwner || project.members.includes(currentUser)
    const sharedNames = sharedDatasetNames(shares, project.name)
    const projectPath = canonicalProjectPath(project.relpath)

    return (
        <>
            <div className="mb-6 flex items-start justify-between gap-4">
                <div>
                    <h1 className="font-mono text-2xl font-bold text-fg">{project.name}</h1>
                    <p className="mt-0.5 text-sm text-fg-subtle">
                        Project owned by {project.owner}
                    </p>
                </div>
                {isOwner && (
                    <Button
                        variant="secondary"
                        onClick={() => navigate(`/projects/${project.name}/members`)}
                        className="whitespace-nowrap"
                    >
                        <UsersIcon className="h-4 w-4"/>
                        Manage members
                    </Button>
                )}
            </div>

            <Card className="mb-8 divide-y divide-default/60">
                <dl className="divide-y divide-default/60">
                    <DetailRow label="Owner">
                        <div className="flex items-center gap-2">
                            <Avatar username={project.owner} size={22}/>
                            <span>{project.owner}</span>
                        </div>
                    </DetailRow>
                    <DetailRow label="Created">{formatDateTime(project.ctime)}</DetailRow>
                    <DetailRow label="Modified">{formatDateTime(project.mtime)}</DetailRow>
                    <DetailRow label="Size">
                        <Suspense fallback={<SizePending/>}>
                            <Await
                                resolve={usage}
                                errorElement={
                                    <span
                                        className="text-fg-faint"
                                        title="The size could not be computed"
                                    >
                                        —
                                    </span>
                                }
                            >
                                <Size/>
                            </Await>
                        </Suspense>
                    </DetailRow>
                    <DetailRow label="Members">
                        {project.members.length === 0 ? (
                            <span className="text-fg-faint">—</span>
                        ) : (
                            <div className="flex flex-wrap items-center gap-2">
                                {project.members.map((m) => (
                                    <span
                                        key={m}
                                        className="inline-flex items-center gap-1.5 rounded-full bg-surface-muted py-0.5 pl-0.5 pr-2.5 text-sm text-fg-muted"
                                    >
                                        <Avatar username={m} size={20}/>
                                        {m}
                                    </span>
                                ))}
                            </div>
                        )}
                    </DetailRow>
                    <DetailRow label="Browse">
                        <div className="flex items-center gap-2">
                            <Link
                                to={browsePath(projectPath)}
                                title="Open in data browser"
                                className="group inline-flex items-center gap-1.5 font-mono text-sm text-primary"
                            >
                                <FolderIcon className="h-3.5 w-3.5 shrink-0"/>
                                <span className="underline decoration-primary/40 underline-offset-2 group-hover:decoration-primary">
                                    {projectPath}
                                </span>
                            </Link>
                            <CopyButton value={browseUrl(projectPath)} label="browse URL"/>
                        </div>
                    </DetailRow>
                </dl>
            </Card>

            <div className="mb-3 flex items-center justify-between">
                <h2 className="text-lg font-bold text-fg">Sessions</h2>
                {canMutate && (
                    <Button
                        onClick={() => navigate(`/projects/${project.name}/sessions/new`)}
                        className="whitespace-nowrap"
                    >
                        <PlusIcon className="h-4 w-4"/>
                        New session
                    </Button>
                )}
            </div>
            <SessionsTable
                project={project}
                datasets={project.sessions}
                sharedNames={sharedNames}
                canMutate={canMutate}
            />
        </>
    )
}

function DetailsSkeleton() {
    const bar = 'animate-pulse rounded bg-surface-muted'
    return (
        <>
            <div className={`${bar} mb-6 h-8 w-64`}/>
            <Card className="mb-8 divide-y divide-default/60">
                {Array.from({length: 6}).map((_, i) => (
                    <div key={i} className="grid grid-cols-[8rem_1fr] gap-4 px-5 py-4">
                        <div className={`${bar} h-4 w-20`}/>
                        <div className={`${bar} h-4 w-48`}/>
                    </div>
                ))}
            </Card>
            <div className={`${bar} mb-3 h-6 w-28`}/>
            <Card className="p-5">
                {Array.from({length: 3}).map((_, i) => (
                    <div key={i} className={`${bar} mb-3 h-5 w-full last:mb-0`}/>
                ))}
            </Card>
        </>
    )
}

/** `<Await>` error boundary — bounce to login on 401, else surface the error. */
function DetailsError({name}: { name: string }) {
    const error = useAsyncError()
    if (error instanceof Unauthorized) {
        return <Navigate to={`/login?next=/projects/${name}`} replace/>
    }
    return (
        <Card className="border-danger/40 bg-danger/5 p-8 text-center">
            <div className="text-base font-semibold text-fg">
                Could not load this project
            </div>
            <div className="mt-1.5 text-sm text-fg-muted">
                {error instanceof Error ? error.message : 'Please try again.'}
            </div>
        </Card>
    )
}

export function ProjectDetails() {
    const {name, details} = useLoaderData() as ProjectDetailsLoaderData

    return (
        <div className="mx-auto max-w-270 px-6 py-8 animate-[fadeIn_0.2s_ease-out]">
            <Breadcrumbs
                className="mb-5"
                crumbs={[
                    {label: 'Projects', to: '/projects'},
                    {label: name, mono: true},
                ]}
            />

            <Suspense fallback={<DetailsSkeleton/>}>
                <Await resolve={details} errorElement={<DetailsError name={name}/>}>
                    <ProjectBody/>
                </Await>
            </Suspense>
        </div>
    )
}
