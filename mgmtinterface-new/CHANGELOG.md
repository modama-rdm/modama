# Changelog

All notable changes to the Data Management portal (new React UI) are recorded here.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).
Versions stay `0.x` while the new UI runs alongside the legacy portal; `1.0.0` is
planned for the cutover.

> This file is also the in-app changelog (footer version badge → **Changelog**
> screen): `src/lib/changelog.ts` reads it at build time. Write entries for the
> people who use the portal. Each release heading ends with a short title for
> the screen: `## [x.y.z] - YYYY-MM-DD - Title`. Notes for developers go under
> `### Internal`, which the screen leaves out, as it does issue references in
> parentheses. Bump `version` in `package.json` in the same commit: the footer
> badge reads it, and a test checks that it matches the newest release here.

## [0.19.2] - 2026-11-06 - Each projects table sorts on its own

### Changed

- On the projects list, a column header now sorts only its own table. “My
  Projects”, “Shared with me” and “All projects” each keep their own order, and
  each order is remembered separately. A click in one table used to sort all
  three. A sort you chose before this version is not carried over: each table
  starts newest first once.

## [0.19.1] - 2026-11-05 - Pages open at the top

### Fixed

- A page you open now starts at the top. Each page used to open at the scroll
  position of the page before it, so the changelog, opened from the version in
  the footer, opened part way down. Back still takes you to where you were.

## [0.19.0] - 2026-11-03 - Descriptions for projects and sessions

### Added

- Describe a project or a session when you create it. The description is saved
  as `README.md` in its folder, where you can also edit it directly. (#112)
- The project page shows the project’s description, with headings, lists and
  links rendered from Markdown. The owner and the members can add one there.
  The `README.md` belongs to whoever writes it first, and only they can change
  it afterwards; an empty description removes it.
- The first line of a description shows under the name in the projects list and
  in a project’s sessions table; hover over it for the whole text. The eye after
  a session’s first line opens the whole description, rendered, with Edit for
  the owner and the members; under a session without one they find “Add
  description”.
- The data browser shows the `README.md` of a project or session folder,
  rendered, above the files in it.
- Both searches look in the descriptions too: “nanowires titan” finds the
  project whose description mentions both.

## [0.18.2] - 2026-11-02 - Sorting keeps your place on the page

### Fixed

- Sorting a project table by clicking a column header no longer makes the page
  jump. With the end of the table above still in view, the page used to scroll
  away with it, often back to “My Projects”; now it stays where it was. (#222)

## [0.18.1] - 2026-10-28 - One “Open in” button per row, and long names that fit

### Changed

- Each row in the data browser has one “Open in” button instead of a badge for
  every app, so the rows stay one line high. The button lists the apps the file
  or folder opens in. Once you have opened something in an app, every row that
  offers it shows that app as a button of its own, so a series of files opens
  in LiberTEM with one click each; the arrow next to it lists the other apps.
  The browser remembers the app you used last. The badges for the folder you
  are in, at the top of the page, stay as they were.

### Fixed

- A file or folder with a very long name no longer stretches the data browser’s
  table and pushes the Size, Modified and “Open in” columns out of view. Long
  names wrap onto more lines, in the table and in the breadcrumb. A project’s
  sessions table does the same for a long session name.

## [0.18.0] - 2026-10-28 - Search a project’s sessions, and search by words

### Added

- A project’s sessions have a search box of their own above their table. Type
  any part of a session’s name: “stem map” finds “2026-09-30 4D-STEM map 01”.
  The box appears once the project has a session. (#113)

### Changed

- The projects search finds a project by any part of its name or its owner.
  More than one word, in any order, finds the projects that have all of them:
  “holo demo” finds “demo-holography”. (#113)
- In both, a dash, an underscore, a dot and a space count as the same, so “demo
  holo” finds “demo-holography” too, and capital letters make no difference.
  Spelling is not corrected: “holgraphy” finds nothing.

### Internal

- Searching in a description comes with the descriptions themselves. (#112)

## [0.17.0] - 2026-10-26 - Sort tables by clicking a column

### Changed

- Click Project, Owner or Last modified at the top of a project table to sort by
  that column, and click it again to reverse the order. All three tables follow
  the same sort. This replaces the “Most recent / Name A–Z” menu next to the
  search box. (#75)
- A project’s sessions sort the same way, by Session, Last modified or Status.
  Status puts a session with an open ingest share first and sealed sessions
  last. (#75)
- The sessions table shows when each session was last modified, the date it has
  always been ordered by, instead of “Created”. That date was not when the
  session was made but the last change to its folder’s settings, such as
  permissions, and many sessions showed the same day.
- Your choices are remembered in this browser, one for the project lists and one
  for the sessions tables, so they open the way you left them.
- Names with numbers in them sort the way you read them: “run2” comes before
  “run10”, and capital letters make no difference.

### Internal

- The old “Created” column showed the folders’ inode change time (`ctime`),
  which any permission change resets. The project page’s “Created” row is
  unchanged for now.

## [0.16.1] - 2026-10-26 - Your password stays out of the browser

### Security

- The portal no longer keeps your password in the browser. It is sent once when
  you sign in, and the session cookie does the rest. A copy an earlier version
  stored is removed the next time you open the portal, without signing you out.
  (#172)

### Fixed

- When your session has expired, the portal shows the sign-in form instead of
  bouncing between it and the page you were on.

### Internal

- The password used to be stored, reversibly encoded, for the legacy API. Every
  screen now uses the new API’s session cookie.

## [0.16.0] - 2026-10-26 - The data browser pages through big folders

### Changed

- Folders are listed 200 entries at a time, with a “Load more” button for the
  rest. Every entry is reachable; the old listing stopped at 1024 and could
  leave out subfolders of a big folder. (#90)
- Entries are ordered folders first, then by name regardless of case.
- A path that does not exist, or a folder you may not read, shows the breadcrumb
  of the path, so any folder above it is one click away.
- The data browser reads the new API, like every other screen. (#172)

### Internal

- A path that does not exist used to show a single link to the nearest existing
  folder, and a folder you may not read a server error.
- The address bar always carries the normalized path: a trailing slash or a
  `..` is replaced before the listing is shown, so copied URLs agree with it.

## [0.15.0] - 2026-10-16 - Project pages open at once

### Changed

- A project page, Manage Members, Seal Session and the ingest share no longer
  wait for the project’s size to be computed before they appear. The project
  page shows the size as soon as it is known.
- Saving members is one step: the server works out who to add and who to
  remove, so if something goes wrong you can simply save again.
- Closing an ingest share that was already closed, for example from another tab,
  no longer shows an error.

### Fixed

- When sealing a session, changing members or closing an ingest share fails, the
  error now says why.

### Internal

- Project details, the user list, changing members, sealing a session, and
  listing and closing ingest shares now go through the new API. The data
  browser is the only screen still on the legacy API. (#172)
- Saving members is a single request, so saving again after an error applies
  only what is still missing. Closing a share that was already closed shows the
  create state.

## [0.14.0] - 2026-10-16 - Creating things is forgiving

### Changed

- Creating a project that you already own, or a session that already exists, is
  no longer an error: the form tells you it is already there and offers to take
  you to it. Opening an ingest share you already have open shows that share
  instead of a second one.
- The project name form now tells you what is wrong with a name (lowercase
  letters, digits, underscores and dashes, 3 to 32 characters, starting with a
  letter) before anything is sent.

### Fixed

- When an ingest share cannot be opened, the error now says why.

### Internal

- Creating a project, a session or an ingest share now goes through the new
  API. (#172)
- The host and workgroup shown for a new ingest share come from the server
  instead of being fixed in the app.

## [0.13.0] - 2026-10-15 - Keep me logged in

### Added

- Added a “Keep me logged in” box on the sign-in screen. Tick it to stay signed
  in for up to 7 days, also across browser restarts, with no inactivity limit.
  It is off by default; leave it off on a shared machine.

### Changed

- Signing in now opens a session that ends after 2 hours without activity, or
  after 12 hours at the latest; after that the sign-in screen appears again.

## [0.12.1] - 2026-07-22 - Stay signed in across tabs

### Changed

- A folder’s info bar now shows an “Open in” link for every app available for
  that location — Nextcloud, JupyterHub, LiberTEM, the image viewer, SampleDB
  and ElabFTW — instead of only Nextcloud and Jupyter.

### Fixed

- Opening a file in a new tab or window no longer makes you sign in again —
  you’re now signed in across every tab and window at once. (#204)
- The light/dark theme no longer breaks the app in private-browsing mode; when
  the browser won’t let it save your choice, it falls back to your system
  setting.

### Internal

- The session used to be tied to a single tab; it is now shared across all tabs
  and windows of the portal.
- The theme falls back the same way whenever browser storage is disabled, and
  the toggle still works for the session.

## [0.12.0] - 2026-07-17 - Shareable browse links

### Added

- Folders now offer “Open in” links for Nextcloud and Jupyter in their info bar.

### Changed

- The copy button in the browser now copies the full link to a folder instead
  of just its path, so you can paste it straight into an ELN entry.
- The copy button on a project’s page copies the same kind of link.

### Fixed

- Browse links are now written exactly like the ones from the old portal, so a
  link to a given folder always looks the same and you can search your ELN for
  it word-for-word.
- Fixed the ElabFTW “Open in” link, which still pointed at the old server
  address.
- Fixed being asked to log in twice: when a saved session had expired, the
  browser’s own login box appeared on top of the portal’s sign-in screen. Only
  the portal’s screen shows now.

### Internal

- Browse URLs keep slashes literal and encode spaces as `+`, so the URL for a
  given path is byte-identical to the legacy portal’s.
- The ElabFTW address is backend configuration: it needs a backend image
  rebuild to take effect in production.
- The double login came from the API’s 401 challenge, which now uses a scheme
  browsers don’t act on, so they no longer show their own Basic-auth dialog.

## [0.11.0] - 2026-07-10 - Dark mode & version history

### Added

- Added a dark theme with a toggle in the header; your choice is remembered and
  follows your system setting by default.
- Added this changelog so you can follow what changes between releases.
- The app version now shows in the footer and links here.

### Internal

- The footer also got its “Made with love at the ER-C” credit.

## [0.10.0] - 2026-07-08 - Data browser

### Added

- Browse project and session files directly in the app, with ready-made links
  to open a folder in Nextcloud, Jupyter, LiberTEM, the image viewer and
  SampleDB.
- Copy-ready rsync, SFTP and SMB commands for each location.
- The modama logo now takes you back to Projects from anywhere.

## [0.9.0] - Ingest share

### Added

- Open and close an ingest share for a session, with copy-ready connection
  details for Windows and Linux.

## [0.8.0] - Manage members

### Added

- Project owners can add and remove members from a project.

## [0.7.0] - Sessions

### Added

- Create a new session (pre-filled with today’s date) and seal a session when
  it’s complete.

## [0.6.0] - New project

### Added

- Create a project and pick its members from a searchable list.

## [0.5.0] - Project details

### Added

- A project overview with its sessions, sizes, status badges and quick actions.

## [0.4.0] - Projects list

### Added

- The projects screen, grouped and sortable, showing the projects you own and
  those shared with you.

## [0.3.0] - App shell & sign-in

### Added

- The signed-in layout and a new login screen.

## [0.1.0] - Foundations

### Added

- First release of the rebuilt Data Management portal.

### Internal

- Scaffold, design tokens, API client and authentication.
