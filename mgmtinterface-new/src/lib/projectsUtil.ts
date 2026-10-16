import type {IngestShareV2, SerializedDataset, SerializedProject} from '../api/types'

export type SortBy = 'recent' | 'name'

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

/** Return a sorted copy: "recent" = newest mtime first, "name" = A - Z. */
export function sortProjects(
    projects: SerializedProject[],
    sortBy: SortBy,
): SerializedProject[] {
    const sorted = [...projects]
    if (sortBy === 'name') {
        sorted.sort((a, b) => a.name.localeCompare(b.name))
    } else {
        sorted.sort((a, b) => b.mtime - a.mtime)
    }
    return sorted
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

/** Return a copy of the datasets sorted newest-modified first (legacy order). */
export function sortDatasets(datasets: SerializedDataset[]): SerializedDataset[] {
    return [...datasets].sort((a, b) => b.mtime - a.mtime)
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
