import type {IngestShareV2, SerializedDataset, SerializedProject} from '../api/types'
import {compareNames, type Sort, type SortSpec} from './sort'

/** The project tables' sortable columns (#75). */
export type ProjectSortKey = 'name' | 'owner' | 'mtime'

export const PROJECT_SORT: SortSpec<ProjectSortKey> = {
    firstDir: {name: 'asc', owner: 'asc', mtime: 'desc'},
    // Newest first, the order the list had before its headers were clickable.
    initial: {key: 'mtime', dir: 'desc'},
}

/** A project's sessions table's sortable columns (#75). */
export type SessionSortKey = 'name' | 'mtime' | 'status'

export const SESSION_SORT: SortSpec<SessionSortKey> = {
    firstDir: {name: 'asc', mtime: 'desc', status: 'asc'},
    // Newest-modified first, the order the table had before its headers were
    // clickable. No "Created": a session's `ctime` is the folders' inode change
    // time, which a bulk permission change resets for every session at once.
    initial: {key: 'mtime', dir: 'desc'},
}

export interface GroupedProjects {
    /** Projects the current user owns. */
    own: SerializedProject[]
    /** Projects the current user is a member of (but doesn't own). */
    member: SerializedProject[]
    /** Everything else. */
    other: SerializedProject[]
}

/**
 * Partition projects into Own / Member / Other for the current user — mirrors
 * the grouping in the legacy `project()` route (`__init__.py`): owner match wins,
 * then membership, else "other".
 */
export function groupProjects(
    projects: SerializedProject[],
    currentUser: string,
): GroupedProjects {
    const own: SerializedProject[] = []
    const member: SerializedProject[] = []
    const other: SerializedProject[] = []
    for (const p of projects) {
        if (p.owner === currentUser) own.push(p)
        else if (p.members.includes(currentUser)) member.push(p)
        else other.push(p)
    }
    return {own, member, other}
}

/** Case-insensitive filter by project name. Empty query returns all. */
export function filterProjects(
    projects: SerializedProject[],
    query: string,
): SerializedProject[] {
    const q = query.trim().toLowerCase()
    if (!q) return projects
    return projects.filter((p) => p.name.toLowerCase().includes(q))
}

/**
 * Return a copy sorted by one column. Ties (same owner, same timestamp) fall
 * back to the name, A - Z whichever way the column runs.
 */
export function sortProjects(
    projects: SerializedProject[],
    {key, dir}: Sort<ProjectSortKey>,
): SerializedProject[] {
    const sign = dir === 'asc' ? 1 : -1
    return [...projects].sort((a, b) => {
        const primary = key === 'mtime' ? a.mtime - b.mtime : compareNames(a[key], b[key])
        return sign * primary || compareNames(a.name, b.name)
    })
}

/** Status order, most active first: ingest share open, then open, then sealed. */
function statusRank(session: SerializedDataset, sharedNames: Set<string>): number {
    if (sharedNames.has(session.name)) return 0
    return session.immutable ? 2 : 1
}

/**
 * Return a copy of a project's sessions sorted by one column, `sharedNames`
 * (from `sharedDatasetNames`) ranking the ones with an ingest share for Status.
 * Ties (same status, same timestamp) fall back to the name, A - Z.
 */
export function sortSessions(
    sessions: SerializedDataset[],
    {key, dir}: Sort<SessionSortKey>,
    sharedNames: Set<string>,
): SerializedDataset[] {
    const sign = dir === 'asc' ? 1 : -1
    return [...sessions].sort((a, b) => {
        const primary =
            key === 'name' ? compareNames(a.name, b.name)
                : key === 'status' ? statusRank(a, sharedNames) - statusRank(b, sharedNames)
                    : a.mtime - b.mtime
        return sign * primary || compareNames(a.name, b.name)
    })
}

/**
 * Canonical filesystem path of a project (or a dataset within it) — mirrors the
 * backend `canonical_path` filter: "/" + relpath [ + "/" + dataset ]. Used for
 * the Browse links and copy-to-clipboard fields.
 */
export function canonicalProjectPath(relpath: string, dataset?: string): string {
    const base = `/${relpath}`
    return dataset ? `${base}/${dataset}` : base
}

/**
 * Names of this project's datasets that currently have an active ingest share.
 * Each share names its project and session (#172), and the list is already
 * scoped to the signed-in user, so this is a plain field match; nothing parses
 * `dest_path` any more.
 */
export function sharedDatasetNames(shares: IngestShareV2[], project: string): Set<string> {
    const names = new Set<string>()
    for (const share of shares) {
        if (share.project === project) names.add(share.session)
    }
    return names
}

/**
 * The active ingest share for one dataset, or `null` if none — the single-share
 * lookup the Ingest screen needs (`sharedDatasetNames` only yields names).
 */
export function findDatasetShare(
    shares: IngestShareV2[],
    project: string,
    dataset: string,
): IngestShareV2 | null {
    return shares.find((s) => s.project === project && s.session === dataset) ?? null
}

/**
 * Diff a project's original members against the edited selection - mirrors the
 * legacy `project_members` route, which computes `to_add`/`to_remove` from the
 * set difference and issues a separate `change_project_members` call for each.
 * Returned lists are sorted for stable ordering in requests/tests.
 */
export function memberChanges(
    original: string[],
    selected: string[],
): { added: string[]; removed: string[] } {
    const before = new Set(original)
    const after = new Set(selected)
    const added = selected.filter((m) => !before.has(m)).sort((a, b) => a.localeCompare(b))
    const removed = original.filter((m) => !after.has(m)).sort((a, b) => a.localeCompare(b))
    return {added, removed}
}

/**
 * Candidate members for the New Project picker: every known user except the
 * owner (implicit member) and those already chosen, optionally narrowed by a
 * case-insensitive name query and returned alphabetically.
 */
export function availableMembers(
    allUsers: string[],
    owner: string,
    selected: string[],
    query: string,
): string[] {
    const taken = new Set([owner, ...selected])
    const q = query.trim().toLowerCase()
    return allUsers
        .filter((u) => !taken.has(u))
        .filter((u) => !q || u.toLowerCase().includes(q))
        .sort((a, b) => a.localeCompare(b))
}
