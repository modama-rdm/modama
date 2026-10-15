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
from .routes import auth, projects, versions
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
""".strip()

TAGS_METADATA = [
    {"name": "auth", "description": "Login, logout, and the current session."},
    {"name": "projects", "description": "Project listings."},
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
