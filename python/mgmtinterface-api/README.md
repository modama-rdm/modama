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

## Layout

    src/mgmtinterface_api/
        __init__.py   app factory, route prefix, healthz
        config.py     Settings (env, MODAMA_ prefix)
        errors.py     APIError + the single error handler
    tests/

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
