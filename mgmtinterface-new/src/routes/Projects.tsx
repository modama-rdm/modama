import {useMemo, useState} from 'react'
import {useLoaderData, useNavigate} from 'react-router-dom'
import type {SerializedProject} from '../api/types'
import {DescriptionDigest} from '../components/DescriptionDigest'
import {Avatar} from '../components/ui/Avatar'
import {Button} from '../components/ui/Button'
import {Card} from '../components/ui/Card'
import {PlusIcon, UploadIcon} from '../components/ui/icons'
import {SearchInput} from '../components/ui/SearchInput'
import {SortHeader} from '../components/ui/SortHeader'
import {useStoredSort} from '../hooks/useStoredSort'
import {formatDate} from '../lib/format'
import {
    filterProjects,
    groupProjects,
    PROJECT_SORT,
    sortProjects,
    type ProjectSortKey,
} from '../lib/projectsUtil.ts'
import type {Sort} from '../lib/sort'
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
                             sort,
                             onSort,
                             onOpen,
                         }: {
    title: string
    projects: SerializedProject[]
    showOwner?: boolean
    sort: Sort<ProjectSortKey>
    onSort: (key: ProjectSortKey) => void
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
                        <SortHeader label="Project" column="name" sort={sort} onSort={onSort} className={thClass}/>
                        {showOwner && (
                            <SortHeader label="Owner" column="owner" sort={sort} onSort={onSort} className={thClass}/>
                        )}
                        <th className={thClass}>Members</th>
                        <SortHeader label="Last modified" column="mtime" sort={sort} onSort={onSort} className={thClass}/>
                    </tr>
                    </thead>
                    {/* A sort moves the rows of all three tables (#222). Kept out of the
                        browser's scroll anchoring, or a row of the table above, anchored at
                        the top of the window, drags the page along to its new place. */}
                    <tbody className="[overflow-anchor:none]">
                    {projects.map((p) => (
                        <tr
                            key={p.name}
                            onClick={() => onOpen(p.name)}
                            className="cursor-pointer border-b border-default/60 last:border-0 hover:bg-surface-muted"
                        >
                            <td className="px-5 py-3.5">
                                <div className="font-mono text-sm font-semibold text-fg">{p.name}</div>
                                <DescriptionDigest description={p.description} className="max-w-[28rem]"/>
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
    const [sort, sortBy] = useStoredSort('erc-projects-sort', PROJECT_SORT)

    const grouped = useMemo(() => {
        const g = groupProjects(filterProjects(projects, query), currentUser)
        return {
            own: sortProjects(g.own, sort),
            member: sortProjects(g.member, sort),
            other: sortProjects(g.other, sort),
        }
    }, [query, sort, currentUser, projects])

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

            <SearchInput
                value={query}
                onChange={(e) => setQuery(e.target.value)}
                placeholder="Search projects…"
                className="mb-7"
            />

            {shares.length > 0 && (
                <div
                    className="mb-7 rounded-xl border border-amber-200 bg-gradient-to-br from-amber-50 to-orange-50 px-5 py-4 dark:border-amber-500/30 dark:from-amber-500/10 dark:to-orange-500/10">
                    <div
                        className="mb-2.5 flex items-center gap-2 text-[11px] font-bold uppercase tracking-wider text-amber-700 dark:text-amber-400">
                        <UploadIcon className="h-3.5 w-3.5"/>
                        Active upload{shares.length === 1 ? '' : 's'} in progress
                    </div>
                    <div className="flex flex-col gap-3">
                        {/* Each share names its project and session (#172); nothing to parse. */}
                        {shares.map((s) => (
                            <div
                                key={s.share_id}
                                className="flex flex-wrap items-center justify-between gap-3"
                            >
                                <div>
                                    <div className="text-sm font-semibold text-fg">
                                        <span className="font-mono">{s.project}</span>
                                        <span className="mx-1.5 font-normal text-fg-faint">/</span>
                                        <span className="font-mono">{s.session}</span>
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
                                                `/projects/${s.project}/sessions/${s.session}/ingest`,
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

            {/* One sort for all three tables, whichever one's header was clicked. */}
            <ProjectsSection
                title="My Projects"
                projects={grouped.own}
                sort={sort}
                onSort={sortBy}
                onOpen={openProject}
            />
            <ProjectsSection
                title="Shared with me"
                projects={grouped.member}
                showOwner
                sort={sort}
                onSort={sortBy}
                onOpen={openProject}
            />
            <ProjectsSection
                title="All projects"
                projects={grouped.other}
                showOwner
                sort={sort}
                onSort={sortBy}
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
