from typing import Any

import pytest
from fastapi import FastAPI
from fastapi.testclient import TestClient
from itsdangerous.timed import TimestampSigner
from mgmtinterface_api.config import Settings
from mgmtinterface_api.session import SessionCodec
from mgmtinterface_api.versioning import API_VERSION_HEADER, LATEST_VERSION


def api_client(app: FastAPI, **kwargs: Any) -> TestClient:
    """
    A client that sends an API version, as every real caller has to.

    The server rejects a request without a version header, so tests that are
    not about versioning would otherwise all fail with a 400. The versioning
    tests build a bare `TestClient`.
    """
    headers = {API_VERSION_HEADER: str(LATEST_VERSION), **kwargs.pop("headers", {})}
    return TestClient(app, headers=headers, **kwargs)


def issue_backdated(
    codec: SessionCodec, monkeypatch: pytest.MonkeyPatch, seconds: int, **kwargs: Any
) -> str:
    """
    A cookie whose itsdangerous stamp lies `seconds` in the past, i.e. one that
    was last refreshed that long ago.

    The stamp has whole-second resolution, so backdating it is what makes idle
    timeouts testable without sleeping.
    """
    real_timestamp = TimestampSigner.get_timestamp
    monkeypatch.setattr(
        TimestampSigner, "get_timestamp", lambda self: real_timestamp(self) - seconds
    )
    try:
        return codec.issue(**kwargs)
    finally:
        monkeypatch.undo()


@pytest.fixture
def settings() -> Settings:
    """
    Settings with the values the tests depend on set explicitly.

    `session_cookie_secure` is off because `TestClient` uses http, and a Secure
    cookie would never be sent back.
    """
    return Settings(
        session_secret="test-secret",
        session_cookie_secure=False,
        session_max_age_seconds=8 * 60 * 60,
        session_idle_seconds=30 * 60,
        session_persistent_max_age_seconds=7 * 24 * 60 * 60,
        session_refresh_seconds=5 * 60,
    )


def signed_in_client(app: FastAPI, settings: Settings, username: str) -> TestClient:
    """A client carrying a valid session cookie for `username`."""
    client = api_client(app)
    client.cookies.set(
        settings.effective_cookie_name, app.state.session_codec.issue(username)
    )
    return client
