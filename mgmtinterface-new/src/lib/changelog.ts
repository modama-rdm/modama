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
        version: '0.18.2',
        date: '2026-11-02',
        title: 'Sorting keeps your place on the page',
        changes: [
            'Sorting a project table by clicking a column header no longer makes the page jump. With the end of the table above still in view, the page used to scroll away with it, often back to “My Projects”; now it stays where it was.',
        ],
    },
    {
        version: '0.18.1',
        date: '2026-10-28',
        title: 'One “Open in” button per row, and long names that fit',
        changes: [
            'Each row in the data browser has one “Open in” button instead of a badge for every app, so the rows stay one line high. The button lists the apps the file or folder opens in. Once you have opened something in an app, every row that offers it shows that app as a button of its own, so a series of files opens in LiberTEM with one click each; the arrow next to it lists the other apps. The browser remembers the app you used last. The badges for the folder you are in, at the top of the page, stay as they were.',
            'A file or folder with a very long name no longer stretches the data browser’s table and pushes the Size, Modified and “Open in” columns out of view. Long names wrap onto more lines, in the table and in the breadcrumb. A project’s sessions table does the same for a long session name.',
        ],
    },
    {
        version: '0.18.0',
        date: '2026-10-28',
        title: 'Search a project’s sessions, and search by words',
        changes: [
            'A project’s sessions have a search box of their own above their table. Type any part of a session’s name: “stem map” finds “2026-09-30 4D-STEM map 01”. The box appears once the project has a session.',
            'The projects search finds a project by any part of its name or its owner. More than one word, in any order, finds the projects that have all of them: “holo demo” finds “demo-holography”.',
            'In both, a dash, an underscore, a dot and a space count as the same, so “demo holo” finds “demo-holography” too, and capital letters make no difference. Spelling is not corrected: “holgraphy” finds nothing.',
        ],
    },
    {
        version: '0.17.0',
        date: '2026-10-26',
        title: 'Sort tables by clicking a column',
        changes: [
            'Click Project, Owner or Last modified at the top of a project table to sort by that column, and click it again to reverse the order. All three tables follow the same sort. This replaces the “Most recent / Name A–Z” menu next to the search box.',
            'A project’s sessions sort the same way, by Session, Last modified or Status. Status puts a session with an open ingest share first and sealed sessions last.',
            'The sessions table shows when each session was last modified, the date it has always been ordered by, instead of “Created”. That date was not when the session was made but the last change to its folder’s settings, such as permissions, and many sessions showed the same day.',
            'Your choices are remembered in this browser, one for the project lists and one for the sessions tables, so they open the way you left them.',
            'Names with numbers in them sort the way you read them: “run2” comes before “run10”, and capital letters make no difference.',
        ],
    },
    {
        version: '0.16.1',
        date: '2026-10-26',
        title: 'Your password stays out of the browser',
        changes: [
            'The portal no longer keeps your password in the browser. It is sent once when you sign in, and the session cookie does the rest. A copy an earlier version stored is removed the next time you open the portal, without signing you out.',
            'When your session has expired, the portal shows the sign-in form instead of bouncing between it and the page you were on.',
        ],
    },
    {
        version: '0.16.0',
        date: '2026-10-26',
        title: 'The data browser pages through big folders',
        changes: [
            'Folders are listed 200 entries at a time, with a “Load more” button for the rest. Every entry is reachable; the old listing stopped at 1024 and could leave out subfolders of a big folder.',
            'Entries are ordered folders first, then by name regardless of case.',
            'A path that does not exist, or a folder you may not read, shows the breadcrumb of the path, so any folder above it is one click away.',
            'The data browser reads the new API, like every other screen.',
        ],
    },
    {
        version: '0.15.0',
        date: '2026-10-16',
        title: 'Project pages open at once',
        changes: [
            'A project page, Manage Members, Seal Session and the ingest share no longer wait for the project’s size to be computed before they appear. The project page shows the size as soon as it is known.',
            'Saving members is one step: the server works out who to add and who to remove, so if something goes wrong you can simply save again.',
            'Closing an ingest share that was already closed, for example from another tab, no longer shows an error.',
            'When sealing a session, changing members or closing an ingest share fails, the error now says why.',
        ],
    },
    {
        version: '0.14.0',
        date: '2026-10-16',
        title: 'Creating things is forgiving',
        changes: [
            'Creating a project that you already own, or a session that already exists, is no longer an error: the form tells you it is already there and offers to take you to it. Opening an ingest share you already have open shows that share instead of a second one.',
            'The project name form now tells you what is wrong with a name (lowercase letters, digits, underscores and dashes, 3 to 32 characters, starting with a letter) before anything is sent.',
            'When an ingest share cannot be opened, the error now says why.',
        ],
    },
    {
        version: '0.13.0',
        date: '2026-10-15',
        title: 'Keep me logged in',
        changes: [
            'Added a “Keep me logged in” box on the sign-in screen. Tick it to stay signed in for up to 7 days, also across browser restarts, with no inactivity limit. It is off by default; leave it off on a shared machine.',
            'Signing in now opens a session that ends after 2 hours without activity, or after 12 hours at the latest; after that the sign-in screen appears again.',
        ],
    },
    {
        version: '0.12.1',
        date: '2026-07-22',
        title: 'Stay signed in across tabs',
        changes: [
            'Opening a file in a new tab or window no longer makes you sign in again — you’re now signed in across every tab and window at once.',
            'The light/dark theme no longer breaks the app in private-browsing mode; when the browser won’t let it save your choice, it falls back to your system setting.',
            'A folder’s info bar now shows an “Open in” link for every app available for that location — Nextcloud, JupyterHub, LiberTEM, the image viewer, SampleDB and ElabFTW — instead of only Nextcloud and Jupyter.',
        ],
    },
    {
        version: '0.12.0',
        date: '2026-07-17',
        title: 'Shareable browse links',
        changes: [
            'The copy button in the browser now copies the full link to a folder instead of just its path, so you can paste it straight into an ELN entry.',
            'Browse links are now written exactly like the ones from the old portal, so a link to a given folder always looks the same and you can search your ELN for it word-for-word.',
            'The copy button on a project’s page copies the same kind of link.',
            'Folders now offer “Open in” links for Nextcloud and Jupyter in their info bar.',
            'Fixed the ElabFTW “Open in” link, which still pointed at the old server address.',
            'Fixed being asked to log in twice: when a saved session had expired, the browser’s own login box appeared on top of the portal’s sign-in screen. Only the portal’s screen shows now.',
        ],
    },
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
