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
import {Unauthorized} from '../api/client'
import type {ProjectDetailsResponse, SerializedDataset} from '../api/types'
import {CopyButton} from '../components/CopyField'
import {Avatar} from '../components/ui/Avatar'
import {Button} from '../components/ui/Button'
import {Card} from '../components/ui/Card'
import {
    ArrowLeftIcon,
    FolderIcon,
    LockIcon,
    PlusIcon,
    UploadIcon,
    UsersIcon,
} from '../components/ui/icons'
import {formatBytes, formatDateTime} from '../lib/format'
import {
    canonicalProjectPath,
    sharedDatasetNames,
    sortDatasets,
} from '../lib/projectsUtil'
import type {ProjectDetailsLoaderData} from './projectDetails.loader'

/** Server-rendered browse route (stays server-side until the FastAPI rewrite). */
function browseHref(path: string): string {
    return `/browse/?path=${encodeURIComponent(path)}`
}

const badgeBase =
    'inline-flex items-center gap-1 rounded-full px-2 py-0.5 text-[11px] font-semibold'

function StatusBadges({dataset, shared}: { dataset: SerializedDataset; shared: boolean }) {
    return (
        <div className="flex flex-wrap items-center gap-1.5">
            {dataset.immutable ? (
                <span className={`${badgeBase} bg-surface-muted text-fg-muted`}>
                    <LockIcon className="h-3 w-3"/>
                    Sealed
                </span>
            ) : (
                <span className={`${badgeBase} bg-primary/10 text-primary`}>Open</span>
            )}
            {shared && (
                <span className={`${badgeBase} bg-warning/15 text-warning`}>
                    <UploadIcon className="h-3 w-3"/>
                    Ingest active
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
    project: ProjectDetailsResponse['project']
    datasets: SerializedDataset[]
    sharedNames: Set<string>
    canMutate: boolean
}) {
    const navigate = useNavigate()
    if (datasets.length === 0) {
        return (
            <Card className="p-10 text-center">
                <div className="text-sm font-semibold text-fg">No sessions yet</div>
                <div className="mt-1 text-sm text-fg-muted">
                    Create a session to start ingesting data into this project.
                </div>
            </Card>
        )
    }
    return (
        <Card className="overflow-hidden">
            <table className="w-full border-collapse">
                <thead>
                <tr className="border-b border-default">
                    <th className={thClass}>Session</th>
                    <th className={thClass}>Modified</th>
                    <th className={thClass}>Status</th>
                    <th className={thClass}/>
                </tr>
                </thead>
                <tbody>
                {datasets.map((d) => {
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
                                <a
                                    href={browseHref(
                                        canonicalProjectPath(project.relpath, d.name),
                                    )}
                                    className="mt-0.5 inline-flex items-center gap-1 text-[11px] text-fg-faint hover:text-primary"
                                >
                                    <FolderIcon className="h-3 w-3"/>
                                    Browse
                                </a>
                            </td>
                            <td className="px-5 py-3.5 text-sm text-fg-muted">
                                {formatDateTime(d.mtime)}
                            </td>
                            <td className="px-5 py-3.5">
                                <StatusBadges dataset={d} shared={shared}/>
                            </td>
                            <td className="px-5 py-3.5">
                                {canMutate && (
                                    <div className="flex items-center justify-end gap-2">
                                        {!d.immutable && (
                                            <Button
                                                variant="secondary"
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
                                        {(!d.immutable || shared) && (
                                            <Button
                                                variant="secondary"
                                                onClick={() =>
                                                    navigate(
                                                        `/projects/${project.name}/sessions/${d.name}/ingest`,
                                                    )
                                                }
                                                className="px-3 py-1.5 text-xs"
                                            >
                                                Manage ingest…
                                            </Button>
                                        )}
                                    </div>
                                )}
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

/** Renders once the slow `details` promise resolves. */
function ProjectBody() {
    const {currentUser, shares} = useLoaderData() as ProjectDetailsLoaderData
    const {project, datasets} = useAsyncValue() as ProjectDetailsResponse
    const navigate = useNavigate()

    const isOwner = currentUser === project.owner
    const canMutate = isOwner || project.members.includes(currentUser)
    const sharedNames = sharedDatasetNames(shares, project.relpath)
    const sorted = sortDatasets(datasets)
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
                        {project.usage_bytes == null ? (
                            <span className="text-fg-faint">—</span>
                        ) : (
                            <span title={`${project.usage_bytes} bytes`}>
                                {formatBytes(project.usage_bytes)}
                            </span>
                        )}
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
                    <DetailRow label="Path">
                        <div className="flex items-center gap-1.5">
                            <a
                                href={browseHref(projectPath)}
                                className="font-mono text-sm text-primary hover:underline"
                            >
                                {projectPath}
                            </a>
                            <CopyButton value={projectPath} label="path"/>
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
                datasets={sorted}
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
            <Link
                to="/projects"
                className="mb-5 inline-flex items-center gap-1.5 text-sm text-fg-subtle hover:text-fg"
            >
                <ArrowLeftIcon className="h-4 w-4"/>
                Projects
            </Link>

            <Suspense fallback={<DetailsSkeleton/>}>
                <Await resolve={details} errorElement={<DetailsError name={name}/>}>
                    <ProjectBody/>
                </Await>
            </Suspense>
        </div>
    )
}
