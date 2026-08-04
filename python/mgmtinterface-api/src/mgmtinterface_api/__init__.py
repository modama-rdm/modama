"""
FastAPI service for the new management interface UI (GitLab #172).

Runs as its own container behind `mgmtinterface-proxy`, which routes `/api/v2/`
here and leaves `/api/` — and every other path — with the legacy Flask app. That
API stays frozen because `modama-agent` depends on it; this one is where
breaking changes (real status codes, cookie sessions, pagination) go.
"""

from fastapi import FastAPI

from .errors import APIError, api_error_handler

API_PREFIX = "/api/v2"


def create_app() -> FastAPI:
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
    )
    app.add_exception_handler(APIError, api_error_handler)

    @app.get(f"{API_PREFIX}/healthz")
    def healthz() -> dict[str, str]:
        """Liveness probe, and the smallest end-to-end check of the route."""
        return {"status": "ok"}

    return app


app = create_app()
