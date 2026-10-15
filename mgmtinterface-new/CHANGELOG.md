# Changelog

All notable changes to the Data Management portal (new React UI) are recorded here.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).
Versions stay `0.x` while the new UI runs alongside the legacy portal; `1.0.0` is
planned for the cutover.

> The in-app changelog (footer version badge → **Changelog** screen) is generated
> from `src/lib/changelog.ts`. Keep the two in sync when you add an entry, and bump
> `version` in `package.json` in the same commit - the footer badge reads that value.

## [0.13.0] - 2026-10-15

### Added

- "Keep me logged in" on the sign-in screen. With the box ticked there is no
  inactivity limit and you stay signed in for up to 7 days, also across browser
  restarts. It is off by default; leave it off on a shared machine.

### Changed

- Signing in now opens a session that ends after 2 hours without activity, or
  after 12 hours at the latest. After that the sign-in screen appears again.

## [0.12.1] - 2026-07-22

### Changed

- A folder's info bar now shows an "Open in" link for every app available for
  that location (Nextcloud, JupyterHub, LiberTEM, the image viewer, SampleDB and
  ElabFTW), matching what files already do, instead of only Nextcloud and Jupyter.

### Fixed

- Opening a file in a new browser tab or window no longer forces a fresh sign-in.
  The session is now shared across all tabs and windows of the portal instead of
  being tied to a single tab. (#204)
- Choosing a light/dark theme no longer takes the app down when browser storage
  is unavailable (e.g. private-browsing mode or storage disabled); the theme
  falls back to the system preference and the toggle still works for the session.

## [0.12.0] - 2026-07-17

### Added

- Folders now show "Open in" links for Nextcloud and Jupyter in their info bar.

### Changed

- The copy button in the data browser copies the full URL of a folder rather than
  its filesystem path, so it can be pasted into an ELN entry. The copy button on
  the project details page does the same.

### Fixed

- Browse URLs are encoded the same way as the legacy portal's (slashes kept
  literal, spaces as `+`), so the URL for a given path is byte-identical between
  the two and stays searchable in ELN entries.
- The ElabFTW "Open in" link pointed at the old server; it now uses the current
  address. Note this is backend configuration - it needs a backend image rebuild
  to take effect in production.
- A stale saved session made the browser show its own Basic-auth dialog on top of
  the portal's sign-in screen, asking for the same credentials twice. The API's
  401 challenge now uses a scheme browsers don't act on, so only the portal's own
  sign-in screen appears.

## [0.11.0] - 2026-07-10

### Added

- Dark theme with a header toggle; the choice is persisted and defaults to the
  system preference.
- This changelog, surfaced in-app on a **Changelog** screen.
- App version shown in the footer, linking to the changelog.
- "Made with love at the ER-C" footer credit.

## [0.10.0] - 2026-07-08

### Added

- Data browser: browse project and session files in-app, with ready-made links
  to Nextcloud, Jupyter, LiberTEM, the image viewer and SampleDB, plus copy-ready
  rsync / SFTP / SMB commands.
- The modama logo links back to Projects from anywhere.

## [0.9.0]

### Added

- Ingest share: open and close a session's ingest share, with copy-ready Windows
  and Linux connection details.

## [0.8.0]

### Added

- Manage members: project owners can add and remove members.

## [0.7.0]

### Added

- Create and seal sessions; new sessions pre-fill today's date.

## [0.6.0]

### Added

- Create a project and pick members from a searchable list.

## [0.5.0]

### Added

- Project details overview with sessions, sizes, status badges and quick actions.

## [0.4.0]

### Added

- Projects list, grouped and sortable, covering owned and shared projects.

## [0.3.0]

### Added

- Signed-in app shell and login screen.

## [0.1.0]

### Added

- First release of the rebuilt Data Management portal (scaffold, design tokens,
  API client and authentication).
