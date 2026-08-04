# mgmtinterface-api

FastAPI service backing the React UI in `mgmtinterface-new/`, tracked as GitLab
#172. It runs as its own container (`mgmtinterface-api`, uvicorn on port 8000)
and `mgmtinterface-proxy` routes **`/api/v2/`** to it.

## Why a second API

The Flask API in `python/mgmtinterface` stays as it is: it is `modama-agent`'s
contract, and the issue is explicit that it keeps working with Basic auth.
Everything that would break it — real HTTP status codes, cookie sessions,
paginated browse — lives here instead. Nothing is removed from Flask by this
service; the two coexist, split by path in nginx, and rolling this back means
deleting one `location` block.

## Conventions

**Status codes, not `{errno}`.** The Flask API answers HTTP 200 with an
`{errno, message}` envelope. This one uses the status code, so clients can rely
on `response.ok`. Errors are raised as a single `APIError` and rendered by a
single handler into:

```json
{"code": "not_a_member", "message": "You are not a member of this project."}
```

`code` is stable and safe to branch on; `message` is safe to display. The
human-readable field is **not** called `detail` on purpose — FastAPI's own 422
validation body uses `detail` for a *list* of per-field errors. 422 keeps
FastAPI's default shape and is the one documented exception to the convention.

**Never send `WWW-Authenticate: Basic`.** It makes the browser pop its own login
dialog over the SPA's login screen; the Flask API needed an explicit workaround
for exactly this. In practice: don't use FastAPI's `HTTPBasic` helper. There is
a test guarding it.

**Configuration** comes from the environment with the `MODAMA_` prefix (see
`config.py`), except secrets, which are read from mounted files.

## Sessions

Login exchanges credentials for a **signed, stateless cookie** —
`__Host-modama_session`, `HttpOnly`, `Secure`, `SameSite=Lax`, `Path=/`. The
name must stay distinct from Flask-Login's `mgmtinterface_session`: both are
live on the same origin until the cutover (#170).

The `__Host-` prefix makes the browser refuse the cookie unless it is `Secure`,
`Path=/` and carries no `Domain` — which is what stops another host under
`fz-juelich.de` from planting a session cookie for this one ("cookie tossing",
a route to session fixation). The prefix requires `Secure`, so plain-http
development falls back to the bare `modama_session`; `Path=/` is a consequence
of the prefix, and is what the cookie will want anyway once the SPA moves to
the root at cutover.

Two lifetimes: **8h absolute** and **30min idle**. The cookie carries the
original login time, so the sliding refresh moves the idle window forward but
can never extend the absolute one. Refreshing happens at most every 5 minutes
(`session_refresh_seconds`), and that is also when the account is re-checked
against the permissions service — pacing it this way keeps that service out of
the per-request path, which was the whole cost of the old Basic scheme.

Known limitations, accepted deliberately:

* **No revocation.** Logout deletes the client's copy; a leaked cookie stays
  valid until it expires and cannot be killed remotely. The short idle lifetime
  bounds this. Revisit at private projects (Theme D), which is when
  admin-killable sessions start to earn a session store.
* **Re-validation is existence-only.** The cookie holds no credentials, so the
  refresh check (`id_user`) confirms the account still resolves — not that it is
  still enabled or that the password has not changed.
* **The signing secret is load-bearing.** With none configured the service
  generates a random one at startup and logs a warning — safe, but sessions
  then end when the container restarts. Set `MODAMA_SESSION_SECRET_FILE` (see
  `docker-compose.prod.yml`) to make them survive. Either way, changing the
  secret signs everybody out. There is deliberately no hardcoded default: a
  *predictable* key would let anyone mint a cookie.

**CSRF:** `SameSite=Lax` plus an `Origin` check on state-changing requests,
falling back to `Referer` when `Origin` is absent, applied app-wide. A request
with *neither* header is allowed — browsers always send one on cross-site
POSTs, so absence means a non-browser caller rather than an attack.

**Brute force:** failed logins are counted per username (case-folded) in
process; past `login_max_failures` within `login_failure_window_seconds` the
endpoint answers 429 with `Retry-After` and stops calling the permissions
service. Only failures count and a success clears the counter, so ordinary
typos never trip it. The trade-off of keying on username alone is that someone
can deliberately throttle a known account; the window is short and nothing is
locked, so it self-heals.

**Not cacheable:** auth responses and every error carry `Cache-Control:
no-store`.

**Upstream failures:** if the permissions service is unreachable, login answers
**502**, never 401 — telling users their password is wrong when the account
service is merely down sends them off resetting credentials that were fine.
Session *refresh* takes the opposite line and keeps the session, since failing
closed there would sign everyone out during a restart of that service.

## Layout

    src/mgmtinterface_api/
        __init__.py   app factory, route prefix, healthz
        main.py       ASGI entry point (`mgmtinterface_api.main:app`)
        config.py     Settings (env, MODAMA_ prefix)
        errors.py     APIError + the single error handler
        session.py    cookie signing/verification, cookie attributes
        security.py   CSRF origin/referer check, current-session dependency
        throttle.py   in-process failed-login counter
        permissions.py  calls into the permissions service
        routes/auth.py  login / logout / session
    tests/

The app is built by `create_app()`; `main.py` is the only place that calls it at
import time. Keeping that out of `__init__.py` means importing the package does
not read configuration or require a session secret.

## Running the tests

Needs `pytest` and `httpx2` (`fastapi.testclient` deprecates plain `httpx`).
These run on macOS — "the backend is Linux-only" is about the full docker stack,
not pytest:

```
python3 -m venv /tmp/venv
/tmp/venv/bin/pip install pytest httpx2 -e ./python/mgmtinterface-api
cd python/mgmtinterface-api && /tmp/venv/bin/pytest tests
```

## Local stack

`docker-compose.override.yml` bind-mounts this directory into the container and
runs uvicorn with `--reload`, so a code change only needs the file synced into
the VM checkout — no rebuild. (The baked image *does* need rebuilding for
production.)
