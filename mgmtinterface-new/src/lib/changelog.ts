import source from '../../CHANGELOG.md?raw'

/**
 * In-app changelog, read from CHANGELOG.md - the only place a release is
 * written down. Vite bundles the file as text (`?raw`) and it is parsed once,
 * when this module loads.
 *
 * The parser reads the subset of Keep a Changelog the file uses:
 *
 *     ## [0.19.1] - 2026-11-05 - Pages open at the top
 *
 *     ### Fixed
 *
 *     - A bullet, which may continue on lines
 *       indented by two spaces.
 *
 * The date may be left out (`## [0.9.0] - Ingest share`). The screen shows a
 * release's bullets in file order, without the `###` headings; the bullets
 * under `### Internal` are notes for developers and are left out, and so are
 * issue references such as the trailing `(#NN)` of a bullet. A heading that is
 * not a version, like `## [Unreleased]`, starts a part the screen skips.
 */
export interface ChangelogEntry {
    version: string
    /** ISO date the version shipped, or null if not recorded. */
    date: string | null
    /** Short headline for the release. */
    title: string
    /** User-facing bullet points, as inline Markdown. */
    changes: string[]
}

const RELEASE = /^## \[(\d+\.\d+\.\d+)\](?: - (\d{4}-\d{2}-\d{2}))?(?: - (.+))?$/
const SECTION = /^### (.+)$/
const ISSUE_REF = /\s*\(#\d+\)/g
/** `###` sections of a release that the screen leaves out. */
const HIDDEN_SECTIONS = new Set(['Internal'])

export function parseChangelog(markdown: string): ChangelogEntry[] {
    const entries: ChangelogEntry[] = []
    let entry: ChangelogEntry | null = null
    let shown = true
    let bullet: string[] | null = null

    const endBullet = () => {
        if (entry && bullet) entry.changes.push(bullet.join(' ').replace(ISSUE_REF, ''))
        bullet = null
    }

    for (const line of markdown.split(/\r?\n/)) {
        const release = RELEASE.exec(line)
        if (release) {
            endBullet()
            entry = {version: release[1], date: release[2] ?? null, title: release[3] ?? '', changes: []}
            entries.push(entry)
            shown = true
            continue
        }
        if (line.startsWith('## ')) {
            endBullet()
            entry = null
            continue
        }
        const section = SECTION.exec(line)
        if (section) {
            endBullet()
            shown = !HIDDEN_SECTIONS.has(section[1].trim())
            continue
        }
        if (!entry || !shown) continue
        if (line.startsWith('- ')) {
            endBullet()
            bullet = [line.slice(2).trim()]
        } else if (bullet && /^ {2}\S/.test(line)) {
            bullet.push(line.trim())
        } else {
            endBullet()
        }
    }
    endBullet()
    return entries
}

/** Releases newest-first, as CHANGELOG.md lists them. */
export const CHANGELOG: ChangelogEntry[] = parseChangelog(source)
