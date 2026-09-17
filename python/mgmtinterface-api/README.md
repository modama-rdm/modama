# mgmtinterface-api

FastAPI service backing the React UI in `mgmtinterface-new/`, tracked as #172.
Runs as its own container (`mgmtinterface-api`, uvicorn on port 8000);
`mgmtinterface-proxy` routes `/mgmt-api/` to it.

## Conventions

Errors use HTTP status codes. A single `APIError` is rendered by a single
handler into:

```json
{"code": "not_a_member", "message": "You are not a member of this project."}
```

`code` is stable and safe to branch on, `message` is safe to display. The
human-readable field is not called `detail` because FastAPI's own 422 validation
body uses `detail` for a list of per-field errors. 422 keeps FastAPI's default
shape and is the one exception to the convention.

Never send `WWW-Authenticate: Basic`. It makes the browser pop its own login
dialog over the SPA's login screen. In practice: don't use FastAPI's `HTTPBasic`
helper. There is a test guarding this.

Configuration comes from the environment with the `MODAMA_` prefix (see
`config.py`). Secrets are read from mounted files.

## Sessions

Login exchanges credentials for a signed, stateless cookie:
`__Host-modama_session`, `HttpOnly`, `Secure`, `SameSite=Lax`, `Path=/`.

`__Host-` is a cookie name prefix defined in RFC 6265bis and enforced by all
current browsers: the browser rejects such a cookie unless it is `Secure`, has
`Path=/` and carries no `Domain`. That stops another host under `fz-juelich.de`
from setting a session cookie for this one (cookie tossing, a route to session
fixation). Because the prefix requires `Secure`, plain-http development falls
back to the bare `modama_session`.

The name has to stay distinct from Flask-Login's `mgmtinterface_session` while
both run on the same origin, until the cutover (#170).

Two lifetimes: 8h absolute and 30min idle. Both values are still under
discussion. The cookie carries the original login time, so the sliding refresh
moves the idle window forward without extending the absolute one. Refresh
happens at most every 5 minutes (`session_refresh_seconds`), which is also when
the account is re-checked against the permissions service. Pacing it this way
keeps that service out of the per-request path.

Accepted limitations:

* No revocation. Logout deletes the client's copy; a leaked cookie stays valid
  until it expires. The idle lifetime bounds this. Revisit at private projects
  (Theme D), where admin-killable sessions need a session store.
* Re-validation is existence-only. The cookie holds no credentials, so the
  refresh check (`id_user`) confirms the account still resolves, not that it is
  still enabled or that the password is unchanged.
* The signing secret has to be configured or the service refuses to start. Set
  `MODAMA_SESSION_SECRET_FILE` (see `docker-compose.prod.yml` and the Secrets
  section of the top-level README). Changing the secret signs everybody out.
  There is no hardcoded default, since a predictable key would let anyone mint a
  cookie. `MODAMA_DEV_MODE=1` substitutes a random per-boot key for development,
  at the cost of sessions not surviving a restart.

CSRF: `SameSite=Lax` plus an `Origin` check on state-changing requests, falling
back to `Referer` when `Origin` is absent, applied app-wide. A request with
neither header is allowed, since browsers always send one on cross-site POSTs.

`csrf_extra_origins` (comma-separated) widens the accepted set. Local
development needs it: the vite dev server on :5180 proxies `/api` to the VM with
`changeOrigin`, so the `Host` the service sees no longer matches the browser's
`Origin` and every POST would 403. Production sets none.

Brute force: failed logins are counted per username (case-folded) in process.
Past `login_max_failures` within `login_failure_window_seconds` the endpoint
answers 429 with `Retry-After` and stops calling the permissions service. Only
failures count, and a success clears the counter. Keying on username alone means
someone can deliberately throttle a known account; the window is short and
nothing is locked, so it self-heals.

Auth responses and errors carry `Cache-Control: no-store`.

If the permissions service is unreachable, login answers 502, not 401. Session
refresh keeps the session instead, so a restart of that service does not sign
everyone out.

## API versions

The version is a header: `X-Modama-API-Version: 1`. The URL never carries one,
which is why the prefix is `/mgmt-api/` and not `/api/v2/`.

Agents are installed on lab machines and updated on the lab's schedule, so the
server has to answer several contracts at once, and a client has to be able to
pin one in a single config value rather than rewriting every URL it builds.

The server never guesses. A missing header is a `400 missing_api_version` and an
unknown version a `400 unsupported_api_version` (400 rather than 404, so a lab
log says why). There is no fallback to the newest version, which would otherwise
move an old client onto a new contract on our next deploy. `/mgmt-api/healthz`
and `/mgmt-api/versions` answer without a version.

Every response carries `X-Modama-API-Versions-Supported`. A versioned response
also echoes `X-Modama-API-Version` and names it in `Vary`, so a shared cache
cannot mix up bodies for the same URL. A deprecated version additionally carries
`Deprecation` (RFC 9745) and `Sunset` (RFC 8594).

To add a version, append to `SUPPORTED_VERSIONS` in `versioning.py` and branch
in a handler on the `api_version` dependency where the shape differs. Additive
changes (a new field, endpoint, or optional request field) do not get a new
version; clients ignore what they do not know, and the Rust structs in
`modama-common` are not `deny_unknown_fields`. How handlers should dispatch once
two versions return incompatible response models is still under discussion.

To remove one: the service logs one line per `(version, client)` pair per boot,
and agents identify themselves as `modama-agent/<git-version>`, so "is any lab
still on version 1" is a log query:

```bash
docker compose logs mgmtinterface-api | grep "First request this boot"
```

A version is removed once that has been empty for a long while and its `Sunset`
date has passed.

Once a version ships, its tests do not change. A behaviour change that makes an
old test fail is the breaking change the version exists to isolate, so it
belongs behind a new number.

## Endpoints

| | |
|---|---|
| `POST /mgmt-api/auth/login` | credentials in, session cookie out |
| `POST /mgmt-api/auth/logout` | 204; clears the cookie |
| `GET /mgmt-api/auth/session` | the signed-in user, or 401 |
| `GET /mgmt-api/projects` | every project, unfiltered |
| `GET /mgmt-api/versions` | which contract versions this server speaks |
| `GET /mgmt-api/healthz` | liveness probe |
| `GET /mgmt-api/docs` | Swagger UI over the generated OpenAPI document |
| `GET /mgmt-api/openapi.json` | that document, for generated clients |

The generated document at `/mgmt-api/docs` is the reference. What makes it
usable is defined alongside the code: request and response models on every
route, the `X-Modama-API-Version` header declared as a parameter
(`versioning.py`) so a generated client sends it, the session cookie declared as
a security scheme (`security.py`), and the `{code, message}` error body attached
per status via `error_responses` (`errors.py`). `/docs` and `/openapi.json` are
unversioned and unauthenticated; nginx forwards the full path, so the prefix
stays intact through the proxy.

`GET /projects` returns `{"projects": [...]}` rather than a bare array, so
pagination fields (Theme A / #90) can be added without breaking callers.
Grouping into own/member/other stays client-side, and access is enforced by the
file system rather than by hiding rows. Private projects (Theme D) is what turns
this into a filtered query.

## Layout

    src/mgmtinterface_api/
        __init__.py   app factory, route prefix, healthz
        main.py       ASGI entry point (`mgmtinterface_api.main:app`)
        config.py     Settings (env, MODAMA_ prefix)
        errors.py     APIError + the single error handler
        session.py    cookie signing/verification, cookie attributes
        security.py   CSRF origin/referer check, current-session dependency
        versioning.py the X-Modama-API-Version contract
        throttle.py   in-process failed-login counter
        permissions.py  calls into the permissions service
        routes/auth.py      login / logout / session
        routes/projects.py  project list
        routes/versions.py  supported versions, unauthenticated
    tests/

The app is built by `create_app()`; `main.py` is the only place that calls it at
import time, so importing the package does not read configuration or require a
session secret.

## Running the tests

Needs `pytest` and `httpx2` (`fastapi.testclient` deprecates plain `httpx`).
These run on macOS; the Linux-only constraint is about the full docker stack,
not pytest:

```
python3 -m venv /tmp/venv
/tmp/venv/bin/pip install pytest httpx2 -e ./python/mgmtinterface-api
cd python/mgmtinterface-api && /tmp/venv/bin/pytest tests
```

## Local stack

`docker-compose.override.yml` bind-mounts this directory into the container and
runs uvicorn with `--reload`, so a code change only needs the file synced into
the VM checkout. The baked image still needs rebuilding for production.
