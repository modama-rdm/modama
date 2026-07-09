import {useMemo, useState} from 'react'
import {useLoaderData, useNavigate} from 'react-router-dom'
import type {SerializedProject} from '../api/types'
import {Avatar} from '../components/ui/Avatar'
import {Button} from '../components/ui/Button'
import {Card} from '../components/ui/Card'
import {PlusIcon, SearchIcon, UploadIcon} from '../components/ui/icons'
import {formatDate} from '../lib/format'
import {
    filterProjects,
    groupProjects,
    parseSharePath,
    sortProjects,
    type SortBy,
} from '../lib/projectsUtil.ts'
import type {ProjectsLoaderData} from './projects.loader'

function MemberAvatars({owner, members}: { owner: string; members: string[] }) {
    const all = [owner, ...members.filter((m) => m !== owner)]
    const shown = all.slice(0, 5)
    const extra = all.length - shown.length
    return (
        <div className="flex items-center">
            {shown.map((m) => (
                <Avatar
                    key={m}
                    username={m}
                    size={26}
                    className="-ml-1.5 ring-2 ring-surface first:ml-0"
                />
            ))}
            {extra > 0 && (
                <div
                    className="-ml-1.5 flex h-6.5 items-center justify-center rounded-full bg-surface-muted px-2 text-[11px] font-semibold text-fg-subtle ring-2 ring-surface">
                    +{extra}
                </div>
            )}
        </div>
    )
}

const thClass =
    'px-5 py-2.5 text-left text-[11px] font-bold uppercase tracking-wide text-fg-faint'

function ProjectsSection({
                             title,
                             projects,
                             showOwner,
                             onOpen,
                         }: {
    title: string
    projects: SerializedProject[]
    showOwner?: boolean
    onOpen: (name: string) => void
}) {
    if (projects.length === 0) return null
    return (
        <section className="mb-8">
            <div className="mb-3 text-[11px] font-bold uppercase tracking-wider text-fg-faint">
                {title}
            </div>
            <Card className="overflow-hidden">
                <table className="w-full border-collapse">
                    <thead>
                    <tr className="border-b border-default">
                        <th className={thClass}>Project</th>
                        {showOwner && <th className={thClass}>Owner</th>}
                        <th className={thClass}>Members</th>
                        <th className={thClass}>Last modified</th>
                    </tr>
                    </thead>
                    <tbody>
                    {projects.map((p) => (
                        <tr
                            key={p.name}
                            onClick={() => onOpen(p.name)}
                            className="cursor-pointer border-b border-default/60 last:border-0 hover:bg-surface-muted"
                        >
                            <td className="px-5 py-3.5">
                                <div className="font-mono text-sm font-semibold text-fg">{p.name}</div>
                                <div className="mt-0.5 text-[11px] text-fg-faint">/{p.relpath}</div>
                            </td>
                            {showOwner && (
                                <td className="px-5 py-3.5">
                                    <div className="flex items-center gap-2">
                                        <Avatar username={p.owner} size={24}/>
                                        <span className="text-sm text-fg-muted">{p.owner}</span>
                                    </div>
                                </td>
                            )}
                            <td className="px-5 py-3.5">
                                <MemberAvatars owner={p.owner} members={p.members}/>
                            </td>
                            <td className="px-5 py-3.5 text-sm text-fg-muted">
                                {formatDate(p.mtime)}
                            </td>
                        </tr>
                    ))}
                    </tbody>
                </table>
            </Card>
        </section>
    )
}

export function Projects() {
    const {currentUser, projects, shares} = useLoaderData() as ProjectsLoaderData
    const navigate = useNavigate()
    const [query, setQuery] = useState('')
    const [sortBy, setSortBy] = useState<SortBy>('recent')

    const grouped = useMemo(() => {
        const g = groupProjects(filterProjects(projects, query), currentUser)
        return {
            own: sortProjects(g.own, sortBy),
            member: sortProjects(g.member, sortBy),
            other: sortProjects(g.other, sortBy),
        }
    }, [query, sortBy, currentUser, projects])

    const ingests = useMemo(
        () => Object.values(shares).map((s) => parseSharePath(s.dest_path)),
        [shares],
    )

    const visibleCount =
        grouped.own.length + grouped.member.length + grouped.other.length
    const openProject = (name: string) => navigate(`/projects/${name}`)

    return (
        <div className="mx-auto max-w-270 px-6 py-8 animate-[fadeIn_0.2s_ease-out]">
            <div className="mb-6 flex items-center justify-between gap-4">
                <div>
                    <h1 className="text-2xl font-bold text-fg">Projects</h1>
                    <p className="mt-0.5 text-sm text-fg-subtle">
                        Your experimental data, organized by project and session.
                    </p>
                </div>
                <Button onClick={() => navigate('/projects/new')} className="whitespace-nowrap">
                    <PlusIcon className="h-4 w-4"/>
                    New Project
                </Button>
            </div>

            <div className="mb-7 flex items-center gap-3">
                <div className="relative flex-1">
                    <SearchIcon
                        className="pointer-events-none absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-fg-faint"/>
                    <input
                        value={query}
                        onChange={(e) => setQuery(e.target.value)}
                        placeholder="Search projects…"
                        className="w-full rounded-lg border border-default bg-surface py-2.5 pl-9 pr-3 text-sm text-fg placeholder:text-fg-faint focus:border-primary focus:outline-none focus:shadow-[0_0_0_3px_var(--ring)]"
                    />
                </div>
                <select
                    value={sortBy}
                    onChange={(e) => setSortBy(e.target.value as SortBy)}
                    className="rounded-lg border border-default bg-surface px-3 py-2.5 text-sm text-fg-muted focus:border-primary focus:outline-none"
                >
                    <option value="recent">Most recent</option>
                    <option value="name">Name A–Z</option>
                </select>
            </div>

            {ingests.length > 0 && (
                <div
                    className="mb-7 rounded-xl border border-amber-200 bg-gradient-to-br from-amber-50 to-orange-50 px-5 py-4 dark:border-amber-500/30 dark:from-amber-500/10 dark:to-orange-500/10">
                    <div
                        className="mb-2.5 flex items-center gap-2 text-[11px] font-bold uppercase tracking-wider text-amber-700 dark:text-amber-400">
                        <UploadIcon className="h-3.5 w-3.5"/>
                        Active upload{ingests.length === 1 ? '' : 's'} in progress
                    </div>
                    <div className="flex flex-col gap-3">
                        {ingests.map((s, i) => (
                            <div
                                key={`${s.project}/${s.dataset}/${i}`}
                                className="flex flex-wrap items-center justify-between gap-3"
                            >
                                <div>
                                    <div className="text-sm font-semibold text-fg">
                                        <span className="font-mono">{s.project}</span>
                                        <span className="mx-1.5 font-normal text-fg-faint">/</span>
                                        <span className="font-mono">{s.dataset}</span>
                                    </div>
                                    <div className="mt-0.5 text-xs text-amber-800 dark:text-amber-200">
                                        Ingest share is open — data is being uploaded to this session
                                    </div>
                                </div>
                                <div className="flex shrink-0 gap-2">
                                    <button
                                        onClick={() => openProject(s.project)}
                                        className="rounded-lg border border-amber-200 bg-surface px-3.5 py-1.5 text-[13px] font-medium text-amber-800 transition-colors hover:bg-amber-50 dark:border-amber-500/30 dark:text-amber-200 dark:hover:bg-amber-500/10"
                                    >
                                        View project
                                    </button>
                                    <button
                                        onClick={() =>
                                            navigate(
                                                `/projects/${s.project}/sessions/${s.dataset}/ingest`,
                                            )
                                        }
                                        className="rounded-lg bg-amber-600 px-4 py-1.5 text-[13px] font-semibold text-white transition-colors hover:bg-amber-700"
                                    >
                                        Manage ingest
                                    </button>
                                </div>
                            </div>
                        ))}
                    </div>
                </div>
            )}

            <ProjectsSection title="My Projects" projects={grouped.own} onOpen={openProject}/>
            <ProjectsSection
                title="Shared with me"
                projects={grouped.member}
                showOwner
                onOpen={openProject}
            />
            <ProjectsSection
                title="All projects"
                projects={grouped.other}
                showOwner
                onOpen={openProject}
            />

            {visibleCount === 0 && (
                <Card className="p-12 text-center">
                    <div className="text-base font-semibold text-fg">No projects found</div>
                    <div className="mt-1.5 text-sm text-fg-muted">
                        {query
                            ? `Nothing matches "${query}".`
                            : 'You have no projects yet.'}
                    </div>
                </Card>
            )}
        </div>
    )
}
