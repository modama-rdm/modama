from typing import Any

import pytest
from fastapi import FastAPI
from fastapi.testclient import TestClient
from mgmtinterface_api.config import Settings
from mgmtinterface_api.versioning import API_VERSION_HEADER, LATEST_VERSION


def api_client(app: FastAPI, **kwargs: Any) -> TestClient:
    """
    A client that pins an API version, the way every real caller has to.

    The server refuses a request that does not say which contract it was built
    against, so tests that are not *about* versioning would otherwise all fail
    on a 400. Tests for the negotiation itself build a bare `TestClient`.
    """
    headers = {API_VERSION_HEADER: str(LATEST_VERSION), **kwargs.pop("headers", {})}
    return TestClient(app, headers=headers, **kwargs)


@pytest.fixture
def settings() -> Settings:
    """
    Settings with the knobs the tests lean on made explicit.

    `session_cookie_secure` is off because `TestClient` speaks http, and a
    Secure cookie would never come back.
    """
    return Settings(
        session_secret="test-secret",
        session_cookie_secure=False,
        session_max_age_seconds=8 * 60 * 60,
        session_idle_seconds=30 * 60,
        session_refresh_seconds=5 * 60,
    )
