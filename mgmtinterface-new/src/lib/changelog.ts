/**
 * In-app changelog data - the single source for the /changelog screen.
 *
 * Keep entries newest-first and written for the people who *use* the portal
 * (what changed for them), not in terms of PR mechanics. When you ship a
 * feature: bump `version` in package.json, add an entry here, and mirror it in
 * the repo-root CHANGELOG.md. The footer badge shows `package.json`'s version
 * (injected as `__APP_VERSION__`), so entry[0].version should match it.
 *
 * Pre-1.0 while the new UI runs alongside the legacy portal; 1.0.0 is planned
 * for the cutover. `date` is ISO (YYYY-MM-DD) or null where the exact ship date
 * predates this changelog - fill those in from `git log` if you want them.
 */
export interface ChangelogEntry {
    version: string
    /** ISO date the version shipped, or null if not recorded. */
    date: string | null
    /** Short headline for the release. */
    title: string
    /** User-facing bullet points. */
    changes: string[]
}

export const CHANGELOG: ChangelogEntry[] = [
    {
        version: '0.11.0',
        date: '2026-07-10',
        title: 'Dark mode & version history',
        changes: [
            'Added a dark theme with a toggle in the header; your choice is remembered and follows your system setting by default.',
            'Added this changelog so you can follow what changes between releases.',
            'The app version now shows in the footer and links here.',
        ],
    },
    {
        version: '0.10.0',
        date: '2026-07-08',
        title: 'Data browser',
        changes: [
            'Browse project and session files directly in the app, with ready-made links to open a folder in Nextcloud, Jupyter, LiberTEM, the image viewer and SampleDB.',
            'Copy-ready rsync, SFTP and SMB commands for each location.',
            'The modama logo now takes you back to Projects from anywhere.',
        ],
    },
    {
        version: '0.9.0',
        date: null,
        title: 'Ingest share',
        changes: [
            'Open and close an ingest share for a session, with copy-ready connection details for Windows and Linux.',
        ],
    },
    {
        version: '0.8.0',
        date: null,
        title: 'Manage members',
        changes: [
            'Project owners can add and remove members from a project.',
        ],
    },
    {
        version: '0.7.0',
        date: null,
        title: 'Sessions',
        changes: [
            'Create a new session (pre-filled with today’s date) and seal a session when it’s complete.',
        ],
    },
    {
        version: '0.6.0',
        date: null,
        title: 'New project',
        changes: [
            'Create a project and pick its members from a searchable list.',
        ],
    },
    {
        version: '0.5.0',
        date: null,
        title: 'Project details',
        changes: [
            'A project overview with its sessions, sizes, status badges and quick actions.',
        ],
    },
    {
        version: '0.4.0',
        date: null,
        title: 'Projects list',
        changes: [
            'The projects screen, grouped and sortable, showing the projects you own and those shared with you.',
        ],
    },
    {
        version: '0.3.0',
        date: null,
        title: 'App shell & sign-in',
        changes: [
            'The signed-in layout and a new login screen.',
        ],
    },
    {
        version: '0.1.0',
        date: null,
        title: 'Foundations',
        changes: [
            'First release of the rebuilt Data Management portal.',
        ],
    },
]
