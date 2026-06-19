# ER-C Data Management — Frontend

React SPA for the ER-C Data Management Self-Service portal ("RDM 2026: UI"). Replaces the
legacy server-rendered Flask + Jinja2 + MUI UI incrementally.

**Stack:** Vite · React + TypeScript · Tailwind CSS v4 · React Router (data mode).

## Develop

```bash
npm install
npm run dev      # Vite dev server (default http://localhost:5173)
```

The backend stack is Linux-only and does not run on macOS, so the dev server **proxies the
API to a running test backend** rather than a local one. Point it at your test server (e.g.
the UTM Debian VM running the test stack) by setting `VITE_API_PROXY`:

```bash
# .env.local  (git-ignored)
VITE_API_PROXY=http://<your-test-vm-ip>:5001
```

`/api`, `/login`, `/logout`, `/browse` and `/static` are proxied there (see
`vite.config.ts`). Defaults to `http://localhost:5001`. **Never point this at production.**

## Build

```bash
npm run build    # type-check (tsc -b) + production bundle into dist/
npm run preview  # serve the built bundle locally
```

Production same-domain serving via Flask/nginx is tracked separately (GitLab #158) and is
not wired up yet.

## Theming

Design tokens are CSS custom properties in [`src/styles/theme.css`](src/styles/theme.css)
(`:root` = light, `.dark` = dark), exposed to Tailwind via `@theme inline`. Components use
semantic utilities only (`bg-surface`, `text-fg`, `border-default`, `bg-primary`, …) and
never hardcode hex — toggling the `dark` class on `<html>` switches the whole theme.
