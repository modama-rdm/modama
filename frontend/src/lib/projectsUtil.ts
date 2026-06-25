import type {SerializedProject} from '../api/types'

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
 * Derive { project, dataset } from a share's `dest_path` — replicates the
 * backend's `split_smbpath`: the dataset is the last path segment, the project
 * the one before it.
 */
export function parseSharePath(destPath: string): {
    project: string
    dataset: string
} {
    const parts = destPath.split('/').filter(Boolean)
    return {
        dataset: parts[parts.length - 1] ?? '',
        project: parts[parts.length - 2] ?? '',
    }
}
