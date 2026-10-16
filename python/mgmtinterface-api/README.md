# mgmtinterface-api

FastAPI service behind the React UI in `mgmtinterface-new/` (#172). It runs as
its own container (`mgmtinterface-api`, uvicorn on port 8000), and
`mgmtinterface-proxy` routes `/mgmt-api/` to it.

## Conventions

Errors are reported through HTTP status codes. Handlers raise `APIError`, and
one exception handler turns it into

```json
{"code": "not_a_member", "message": "You are not a member of this project."}
```

Clients can branch on `code`, which is stable. `message` is meant for display.
The field is not called `detail` because FastAPI's own 422 validation response
uses `detail` for a list of per-field errors. 422 responses keep FastAPI's
default shape; they are the only exception.

Do not send `WWW-Authenticate: Basic`. The browser would open its own login
dialog on top of the SPA's login screen. This rules out FastAPI's `HTTPBasic`
helper. A test checks that the header is absent.

Configuration is read from the environment with the `MODAMA_` prefix (see
`config.py`). Secrets are read from mounted files.

## Sessions

On login the service sets a signed, stateless session cookie:
`__Host-modama_session`, `HttpOnly`, `Secure`, `SameSite=Lax`, `Path=/`.

`__Host-` is a cookie name prefix defined in RFC 6265bis. Current browsers
reject a cookie with this prefix unless it is `Secure`, has `Path=/` and has no
`Domain`. As a result, another host under `fz-juelich.de` cannot set a session
cookie for this service (cookie tossing, which would allow session fixation).
Since the prefix requires `Secure`, plain-http development uses the name
`modama_session` instead.

The name must differ from Flask-Login's `mgmtinterface_session` as long as both
run on the same origin, i.e. until the cutover (#170).

A session has two lifetimes, 12h absolute and 2h idle, configurable with
`MODAMA_SESSION_MAX_AGE_SECONDS` and `MODAMA_SESSION_IDLE_SECONDS`. The cookie
stores the original login time, so refreshing it extends the idle window but
not the absolute one. The cookie is refreshed at most every 5 minutes
(`session_refresh_seconds`), and the account is re-checked against the
permissions service at the same time. The permissions service is therefore not
called on every request.

A login with `"persistent": true` ("keep me logged in") opens a session with no
idle timeout and a 7-day absolute limit, configurable with
`MODAMA_SESSION_PERSISTENT_MAX_AGE_SECONDS`. It is meant for the UI's checkbox
and for long-running clients such as modama-agent. The flag is stored in the
signed cookie, so a refresh keeps the kind of session, and the cookie's
`Max-Age` is the persistent limit, so the browser keeps it across restarts.
Re-validation against the permissions service still happens every 5 minutes,
which is what ends a persistent session once the account is gone. The flag is
off by default, and so is the checkbox. `POST /auth/login` and
`GET /auth/session` both report the kind of session as `persistent`.

Known limitations:

* There is no revocation. Logout deletes the client's copy of the cookie, but a
  leaked cookie stays valid until it expires. The idle lifetime limits the
  damage for a normal session. A persistent cookie has no idle lifetime, so a
  leaked one, for example from a shared lab machine, stays usable for up to 7
  days. This has to be revisited for private projects (Theme D), where an
  admin must be able to end a session, which needs a session store.
* Re-validation only checks that the account exists. The cookie holds no
  credentials, so the refresh check (`id_user`) confirms that the account still
  resolves, not that it is still enabled or that the password is unchanged.
* The signing secret must be configured, otherwise the service refuses to
  start. Set `MODAMA_SESSION_SECRET_FILE` (see `docker-compose.prod.yml` and the
  Secrets section of the top-level README). Changing the secret invalidates all
  sessions. There is no hardcoded default, because a known key would let anyone
  forge a cookie. With `MODAMA_DEV_MODE=1` the service generates a random key at
  startup for development; sessions then do not survive a restart.

CSRF protection consists of `SameSite=Lax` and an `Origin` check on
state-changing requests, which falls back to `Referer` when `Origin` is absent.
The check applies to the whole app. A request with neither header is allowed,
because browsers always send one of them on cross-site POSTs.

`csrf_extra_origins` (comma-separated) adds further accepted origins. Local
development needs it: the vite dev server on :5180 proxies `/api` to the VM with
`changeOrigin`, so the `Host` header the service sees does not match the
browser's `Origin`, and every POST would get a 403. Production does not set it.

Failed logins are counted per username (case-folded) in memory. After
`login_max_failures` failures within `login_failure_window_seconds`, the login
endpoint answers 429 with `Retry-After` and stops calling the permissions
service. Only failures are counted, and a successful login clears the counter.
Because the counter is keyed on the username alone, someone can deliberately
trigger the throttle for a known account. The window is short and no account
is locked, so the state clears on its own.

Auth responses and error responses carry `Cache-Control: no-store`.

If the permissions service is unreachable, login answers 502 rather than 401.
Session refresh keeps the existing session in that case, so a restart of the
permissions service does not log everyone out.

## API versions

The API version is sent in a header, `X-Modama-API-Version: 1`, and never in
the URL. That is why the prefix is `/mgmt-api/` rather than `/api/v2/`.

Agents are installed on lab machines and updated on the lab's schedule, so the
server has to support several contract versions at once, and a client must be
able to pin one in a single config value instead of changing every URL it
builds.

The header is mandatory. A missing header results in `400 missing_api_version`
and an unknown version in `400 unsupported_api_version`. These are 400 rather
than 404 so that the reason is visible in the client's log. There is no
fallback to the newest version, because that would silently move an old client
onto a new contract at the next deploy. `/mgmt-api/healthz` and
`/mgmt-api/versions` do not require the header.

Every response carries `X-Modama-API-Versions-Supported`. A versioned response
also echoes `X-Modama-API-Version` and lists it in `Vary`, so a shared cache
cannot serve the body of one version for a request of another. A deprecated
version additionally carries `Deprecation` (RFC 9745) and `Sunset` (RFC 8594).

To add a version, append it to `SUPPORTED_VERSIONS` in `versioning.py` and
branch on the `api_version` dependency in the handlers whose response shape
differs. Additive changes (a new field, a new endpoint, a new optional request
field) do not get a new version. Clients are expected to ignore unknown fields;
the Rust structs in `modama-common` are not `deny_unknown_fields`. How handlers
should dispatch once two versions return incompatible response models is still
open (#212).

The service logs one line per `(version, client)` pair per boot, and agents
identify themselves as `modama-agent/<git-version>`, so the log shows which
versions are still in use:

```bash
docker compose logs mgmtinterface-api | grep "First request this boot"
```

A version can be removed once this has shown no clients on it for a while and
its `Sunset` date has passed.

The tests of a released version are not changed. If a behaviour change makes an
old test fail, that change is breaking and belongs in a new version.

## Endpoints

| | |
|---|---|
| `POST /mgmt-api/auth/login` | takes credentials and an optional `persistent` flag, sets the session cookie |
| `POST /mgmt-api/auth/logout` | clears the cookie, answers 204 |
| `GET /mgmt-api/auth/session` | the signed-in user, or 401 |
| `GET /mgmt-api/projects` | all projects, unfiltered |
| `POST /mgmt-api/projects` | create a project owned by the signed-in user |
| `POST /mgmt-api/projects/{project}/sessions` | create a session in a project |
| `POST /mgmt-api/ingest-shares` | open an ingest share for a session |
| `GET /mgmt-api/versions` | supported API versions |
| `GET /mgmt-api/healthz` | liveness probe |
| `GET /mgmt-api/docs` | Swagger UI for the generated OpenAPI document |
| `GET /mgmt-api/openapi.json` | the OpenAPI document, for generating clients |

The OpenAPI document is the reference for the API. It is complete because the
relevant parts are declared in code: every route has request and response
models, `versioning.py` declares the `X-Modama-API-Version` header as a
parameter so that generated clients send it, `security.py` declares the session
cookie as a security scheme, and `errors.py` attaches the `{code, message}`
error body to each status code via `error_responses`. `/docs` and
`/openapi.json` need neither a version header nor a session. nginx forwards the
full path, so the `/mgmt-api/` prefix is preserved behind the proxy.

`GET /projects` returns `{"projects": [...]}` instead of a bare array, so that
pagination fields (Theme A / #90) can be added later without breaking callers.
Grouping into own/member/other happens in the client. Access is enforced by the
file system, not by hiding rows. This changes with private projects (Theme D),
which need a filtered query.

## Creating things

The three `POST` endpoints are named after the UI's terms, session and ingest
share, rather than the legacy API's dataset and share. They are idempotent:
the agent retries on flaky networks and may never have seen the answer to its
first attempt, so a repeat of the same request must not fail.

| | first request | repeat |
|---|---|---|
| `POST /projects` | 201, the new project | 200, the existing project, unchanged |
| `POST /projects/{project}/sessions` | 201, the new session | 200, the existing session |
| `POST /ingest-shares` | 201, a new share | 200, the share the user already has for that session |

Both answers carry the resource; a client treats any 2xx as "I have it". The
one conflict is `409 project_exists`: a project of that name that belongs to
someone else. A repeat with a different member list does not change the
members of an existing project; that is a separate operation.

How each one tells "already there" from "refused":

* Project: the permissions service refuses an existing name with the same
  `errno` it uses for an invalid one, so the create is attempted first and the
  name looked up only on refusal. Looking first would leave a window in which
  two concurrent requests both see nothing and one of them fails with the very
  error the endpoint absorbs. A refusal for a name that does not exist is a
  `400 project_rejected` with the service's message.
* Session: the project is read first (membership is checked on the same read,
  giving a `403 not_a_member` instead of a `mkdir` error); an existing session
  is returned, otherwise the folder is created and the project read again for
  the session's timestamps. A refusal is re-checked against the project once,
  for the case where a concurrent request created the session in between.
* Ingest share: smbdrop opens a second share for the same path without
  complaint, so the user's open shares are listed and matched on the
  normalized path and uid, like the legacy `add_share_info`. Another user's
  share for the same session does not count; each user ingests under their
  own identity.

The project and session name rules of the permissions service
(`check_valid_group`, `check_valid_dataset`) are repeated as request-model
constraints, so that they are in the OpenAPI document and a bad name fails
with a 422 before a round trip. The service stays the authority for everything
it checks beyond the spelling (Unix groups, existing folders, who may create).

The ingest share response adds what the legacy response left to the client:
`project`, `session`, and the `smb_host` and `workgroup` to connect with
(`MODAMA_INGEST_SMB_HOST`, `MODAMA_INGEST_WORKGROUP`). Not in this version:
the `X-Agent-Token` path of the legacy endpoint, which binds a share to the
instrument an agent runs on (`resource`). That comes with the agent's move to
this API, together with the question of how the agent authenticates.

The owner of a new project is the signed-in user. The legacy request carried an
`owner` field that had to equal the authenticated user; here the session is the
only source.

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
        shares.py       calls into smbdrop, the share service
        routes/auth.py      login / logout / session
        routes/projects.py  project list, create project
        routes/sessions.py  create session
        routes/ingest_shares.py  open ingest share
        routes/versions.py  supported versions, unauthenticated
    tests/

The app is created by `create_app()`. `main.py` is the only module that calls
it at import time, so importing the package neither reads configuration nor
requires a session secret.

## Running the tests

The tests need `pytest` and `httpx2` (`fastapi.testclient` deprecates plain
`httpx`). They run on macOS; only the full docker stack is Linux-only:

```
python3 -m venv /tmp/venv
/tmp/venv/bin/pip install pytest httpx2 ./python/permissions-client ./python/smbdrop-client -e ./python/mgmtinterface-api
cd python/mgmtinterface-api && /tmp/venv/bin/pytest tests
```

## Local stack

`docker-compose.override.yml` bind-mounts this directory into the container and
runs uvicorn with `--reload`. A code change therefore only has to be synced into
the VM checkout. For production the image has to be rebuilt.
