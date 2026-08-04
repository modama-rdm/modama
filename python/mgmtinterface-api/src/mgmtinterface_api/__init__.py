"""
FastAPI service for the new management interface UI (GitLab #172).

Runs as its own container behind `mgmtinterface-proxy`, which routes `/api/v2/`
here and leaves `/api/` — and every other path — with the legacy Flask app. That
API stays frozen because `modama-agent` depends on it; this one is where
breaking changes (real status codes, cookie sessions, pagination) go.
"""

from fastapi import Depends, FastAPI

from .config import Settings, get_settings
from .errors import APIError, api_error_handler
from .routes import auth, projects
from .security import enforce_same_origin
from .session import SessionCodec
from .throttle import FailureThrottle

API_PREFIX = "/api/v2"


def create_app(settings: Settings | None = None) -> FastAPI:
    settings = settings or get_settings()
    app = FastAPI(
        title="modama API",
        version="0.1.0",
        # nginx forwards the full path, so the docs have to live under the
        # prefix too or they 404 through the proxy.
        docs_url=f"{API_PREFIX}/docs",
        openapi_url=f"{API_PREFIX}/openapi.json",
        redoc_url=None,
        # Would otherwise be registered at /docs/oauth2-redirect, outside the
        # prefix nginx routes here — and this API has no OAuth flow anyway.
        swagger_ui_oauth2_redirect_url=None,
        # App-wide rather than per-router: every state-changing endpoint needs
        # the CSRF check, and this way a new router cannot omit it.
        dependencies=[Depends(enforce_same_origin)],
    )
    app.add_exception_handler(APIError, api_error_handler)

    app.state.settings = settings
    # Resolved eagerly: a missing secret should stop the container from starting
    # rather than surface as a failed login later.
    app.state.session_codec = SessionCodec(
        settings.resolve_session_secret(),
        max_age_seconds=settings.session_max_age_seconds,
        idle_seconds=settings.session_idle_seconds,
    )
    app.state.login_throttle = FailureThrottle(
        max_failures=settings.login_max_failures,
        window_seconds=settings.login_failure_window_seconds,
    )

    app.include_router(auth.router, prefix=API_PREFIX)
    app.include_router(projects.router, prefix=API_PREFIX)

    @app.get(f"{API_PREFIX}/healthz")
    def healthz() -> dict[str, str]:
        """Liveness probe, and the smallest end-to-end check of the route."""
        return {"status": "ok"}

    return app
