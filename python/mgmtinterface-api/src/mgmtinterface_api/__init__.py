"""
FastAPI service for the new management interface UI (GitLab #172).

Runs as its own container behind `mgmtinterface-proxy`, which routes
`/mgmt-api/` here and everything else, including `/api/`, to the Flask app.
The Flask API stays frozen because `modama-agent` depends on it; breaking
changes (real status codes, cookie sessions, pagination) go into this service.

The API is versioned by request header rather than by path, so locally
installed clients can stay on an older contract; see `versioning.py`.
"""

from typing import Any

from fastapi import Depends, FastAPI

from .config import API_PREFIX, Settings, get_settings
from .errors import APIError, api_error_handler
from .routes import auth, browse, ingest_shares, projects, sessions, users, versions
from .security import enforce_same_origin
from .session import SessionCodec
from .throttle import FailureThrottle
from .versioning import (
    API_VERSION_HEADER,
    LATEST_VERSION,
    add_version_headers,
    document_version_header,
    require_api_version,
    supported_versions_header,
)

__all__ = ["API_PREFIX", "create_app"]

# Shown at the top of the generated documentation. The rules a client author
# needs to know are stated here and not only in the source, because the
# document is what the author of `modama-agent` or of an instrument script will
# read.
DESCRIPTION = f"""
The API behind the new management interface. The legacy `/api/` endpoints are
a separate API, which stays frozen for existing clients.

### Versioning

Every request must state the contract version it was written against, as a bare
integer in the `{API_VERSION_HEADER}` header. There is no default: a request
without the header is rejected, so a deploy cannot silently move an installed
client onto a newer contract. Currently supported: {{versions}}.

The resolved version is echoed on the response, together with
`X-Modama-API-Versions-Supported`. A deprecated version is announced with
`Deprecation` and `Sunset` headers before it is switched off. `GET /versions`
lists the same information and can be called before pinning a version.

### Errors

Errors use HTTP status codes with a body of `{{{{"code", "message"}}}}`. Clients
can branch on `code`, which is stable; `message` is meant for display. The one
exception is HTTP 422, which carries FastAPI's own field-level `detail` list for
a malformed request body.

### Authentication

`POST /auth/login` sets an `HttpOnly` session cookie, which the browser sends
back automatically. A session ends after a period of inactivity and after an
absolute limit; `"persistent": true` on login drops the idle timeout in exchange
for a longer absolute limit ("keep me logged in"). State-changing requests are
also checked against the request `Origin`. Endpoints that need a session are
marked with a lock below.

### Creating things

The `POST` endpoints that create a project, a session or an ingest share are
idempotent: the first request answers `201` with the new resource, and a repeat
of the same request answers `200` with the existing one instead of an error. A
client that lost a response to a flaky network can therefore simply send the
request again. The only conflict (`409`) is a project of the same name that
belongs to someone else.

### Changing things

`PUT /projects/{{{{project}}}}/members` takes the complete member list wanted,
not operations; the API works out what to add and remove, so a retry is safe.
Sealing a session that is sealed already answers `200` with the session as it
is. `DELETE /ingest-shares/{{{{share_id}}}}` answers `204`, and `404` once the
share is gone.

A project or session description is the text of a `README.md` in its folder,
`null` when there is none. It can be given on create and changed with
`PUT .../description`; an empty text removes the file. The file is written over
WebDAV as the signed-in user, so the file system decides who may change it:
whoever created it. People can also edit the file directly; whatever it says is
shown.

### Slow calls

`GET /projects/{{{{project}}}}/usage` runs `du` over the project and can take a
minute on a large one. Nothing else waits for it: the project details answer
with `usage_bytes` null.

### Browsing

`GET /browse?path=…` lists a folder of the data tree one page at a time:
`entries` holds up to `limit` of them (200 by default, 1000 at most), folders
first and then by name, and `next_cursor` fetches the page after it, until it
is null. The cursor marks the last entry seen rather than an offset, so an
entry added or removed while paging neither repeats nor hides another. The
listing is made as the signed-in user and shows what that user may see.
""".strip()

TAGS_METADATA = [
    {"name": "auth", "description": "Login, logout, and the current session."},
    {
        "name": "projects",
        "description": "Projects: list, details, disk usage, members, creation.",
    },
    {"name": "users", "description": "The accounts a project can have as members."},
    {
        "name": "sessions",
        "description": (
            "Sessions: the session folders of a project, each meant for one "
            "stretch of work at one resource. The legacy API calls them "
            "datasets."
        ),
    },
    {
        "name": "ingest shares",
        "description": (
            "Temporary Samba shares with one-time credentials for writing data "
            "into a session folder from an instrument computer. The legacy API "
            "calls them shares."
        ),
    },
    {
        "name": "browse",
        "description": (
            "The data tree, as the signed-in user sees it: a folder's entries "
            "one page at a time, with the links to open a path elsewhere."
        ),
    },
    {
        "name": "versions",
        "description": (
            "Supported contract versions. Unauthenticated and unversioned, so a "
            "client can call it before pinning a version."
        ),
    },
]


def create_app(settings: Settings | None = None) -> FastAPI:
    settings = settings or get_settings()
    app = FastAPI(
        title="modama API",
        # The contract version this document describes, not a release number.
        # This is the integer a client pins in the X-Modama-API-Version header.
        version=str(LATEST_VERSION),
        summary="Management interface API for the ER-C data management system.",
        description=DESCRIPTION.format(versions=supported_versions_header()),
        openapi_tags=TAGS_METADATA,
        # nginx forwards the full path, so the docs have to live under the
        # prefix too or they 404 through the proxy.
        docs_url=f"{API_PREFIX}/docs",
        openapi_url=f"{API_PREFIX}/openapi.json",
        redoc_url=None,
        # Would otherwise be registered at /docs/oauth2-redirect, outside the
        # prefix nginx routes here. This API has no OAuth flow anyway.
        swagger_ui_oauth2_redirect_url=None,
        # App-wide rather than per router: every state-changing endpoint needs
        # the CSRF check and every endpoint needs a resolved API version. This
        # way a router added later cannot omit either.
        dependencies=[Depends(enforce_same_origin), Depends(require_api_version)],
    )
    app.add_exception_handler(APIError, api_error_handler)
    # Middleware rather than a dependency, because it has to reach the
    # responses built by exception handlers as well.
    _ = app.middleware("http")(add_version_headers)

    app.state.settings = settings
    # Resolved at startup, so a missing secret stops the container from
    # starting instead of showing up as a failed login later.
    app.state.session_codec = SessionCodec(
        settings.resolve_session_secret(),
        max_age_seconds=settings.session_max_age_seconds,
        idle_seconds=settings.session_idle_seconds,
        persistent_max_age_seconds=settings.session_persistent_max_age_seconds,
    )
    # (version, client) pairs seen since boot, used to tell whether an old
    # version is still in use; see `versioning.py`.
    app.state.api_version_clients = set()
    app.state.login_throttle = FailureThrottle(
        max_failures=settings.login_max_failures,
        window_seconds=settings.login_failure_window_seconds,
    )

    app.include_router(auth.router, prefix=API_PREFIX)
    app.include_router(projects.router, prefix=API_PREFIX)
    app.include_router(users.router, prefix=API_PREFIX)
    app.include_router(sessions.router, prefix=API_PREFIX)
    app.include_router(ingest_shares.router, prefix=API_PREFIX)
    app.include_router(browse.router, prefix=API_PREFIX)
    app.include_router(versions.router, prefix=API_PREFIX)

    @app.get(
        f"{API_PREFIX}/healthz",
        tags=["versions"],
        summary="Liveness probe",
    )
    def healthz() -> dict[str, str]:
        """Liveness probe."""
        return {"status": "ok"}

    # The generated document needs one correction that the route decorators
    # cannot express; see `document_version_header`. FastAPI caches the result
    # in `openapi_schema`, so this runs once.
    generate = app.openapi

    def openapi() -> dict[str, Any]:
        return document_version_header(generate())

    app.openapi = openapi

    return app
