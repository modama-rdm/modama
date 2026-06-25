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

function MemberAvatars({members}: { members: string[] }) {
    if (members.length === 0) return <span className="text-sm text-fg-faint">—</span>
    const shown = members.slice(0, 5)
    const extra = members.length - shown.length
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
                                <MemberAvatars members={p.members}/>
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
    }, [query, sortBy])

    const ingests = useMemo(
        () => Object.values(shares).map((s) => parseSharePath(s.dest_path)),
        [],
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
                <Card className="mb-7 border-warning/40 bg-warning/5 p-5">
                    <div
                        className="mb-2 flex items-center gap-2 text-[11px] font-bold uppercase tracking-wider text-warning">
                        <UploadIcon className="h-3.5 w-3.5"/>
                        Active ingest {ingests.length === 1 ? 'session' : 'sessions'}
                    </div>
                    <div className="flex flex-col gap-2">
                        {ingests.map((s, i) => (
                            <div
                                key={`${s.project}/${s.dataset}/${i}`}
                                className="flex items-center justify-between gap-3"
                            >
                                <div className="text-sm text-fg">
                                    <span className="font-mono font-semibold">{s.project}</span>
                                    <span className="mx-1.5 text-fg-faint">/</span>
                                    <span className="font-mono">{s.dataset}</span>
                                </div>
                                <Button
                                    variant="secondary"
                                    onClick={() => openProject(s.project)}
                                    className="px-3 py-1.5 text-xs"
                                >
                                    View project
                                </Button>
                            </div>
                        ))}
                    </div>
                </Card>
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
