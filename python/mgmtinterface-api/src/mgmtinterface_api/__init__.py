"""
FastAPI service for the new management interface UI (GitLab #172).

Runs as its own container behind `mgmtinterface-proxy`, which routes
`/mgmt-api/` here and leaves `/api/` — and every other path — with the legacy
Flask app. That API stays frozen because `modama-agent` depends on it; this one
is where breaking changes (real status codes, cookie sessions, pagination) go.

Those breaking changes are versioned by request header rather than by path, so
locally installed clients can stay on an older contract while the server moves
on; see `versioning.py`.
"""

from fastapi import Depends, FastAPI

from .config import API_PREFIX, Settings, get_settings
from .errors import APIError, api_error_handler
from .routes import auth, projects, versions
from .security import enforce_same_origin
from .session import SessionCodec
from .throttle import FailureThrottle
from .versioning import LATEST_VERSION, add_version_headers, require_api_version

__all__ = ["API_PREFIX", "create_app"]


def create_app(settings: Settings | None = None) -> FastAPI:
    settings = settings or get_settings()
    app = FastAPI(
        title="modama API",
        # The contract version this document describes. Not a release number:
        # what a client pins is this integer, in the X-Modama-API-Version header.
        version=str(LATEST_VERSION),
        # nginx forwards the full path, so the docs have to live under the
        # prefix too or they 404 through the proxy.
        docs_url=f"{API_PREFIX}/docs",
        openapi_url=f"{API_PREFIX}/openapi.json",
        redoc_url=None,
        # Would otherwise be registered at /docs/oauth2-redirect, outside the
        # prefix nginx routes here — and this API has no OAuth flow anyway.
        swagger_ui_oauth2_redirect_url=None,
        # App-wide rather than per-router: every state-changing endpoint needs
        # the CSRF check, and every endpoint needs a resolved API version, and
        # this way a router added later cannot omit either.
        dependencies=[Depends(enforce_same_origin), Depends(require_api_version)],
    )
    app.add_exception_handler(APIError, api_error_handler)
    # Middleware, not a dependency: this has to reach the responses that
    # exception handlers produce as well.
    _ = app.middleware("http")(add_version_headers)

    app.state.settings = settings
    # Resolved eagerly: a missing secret should stop the container from starting
    # rather than surface as a failed login later.
    app.state.session_codec = SessionCodec(
        settings.resolve_session_secret(),
        max_age_seconds=settings.session_max_age_seconds,
        idle_seconds=settings.session_idle_seconds,
    )
    # Which (version, client) pairs have been seen since boot — the evidence for
    # whether an old version is still in use anywhere; see `versioning.py`.
    app.state.api_version_clients = set()
    app.state.login_throttle = FailureThrottle(
        max_failures=settings.login_max_failures,
        window_seconds=settings.login_failure_window_seconds,
    )

    app.include_router(auth.router, prefix=API_PREFIX)
    app.include_router(projects.router, prefix=API_PREFIX)
    app.include_router(versions.router, prefix=API_PREFIX)

    @app.get(f"{API_PREFIX}/healthz")
    def healthz() -> dict[str, str]:
        """Liveness probe, and the smallest end-to-end check of the route."""
        return {"status": "ok"}

    return app
