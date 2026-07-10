# Changelog

All notable changes to the Data Management portal (new React UI) are recorded here.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).
Versions stay `0.x` while the new UI runs alongside the legacy portal; `1.0.0` is
planned for the cutover.

> The in-app changelog (footer version badge → **Changelog** screen) is generated
> from `src/lib/changelog.ts`. Keep the two in sync when you add an entry, and bump
> `version` in `package.json` in the same commit - the footer badge reads that value.

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
