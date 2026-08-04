import pytest
from mgmtinterface_api.config import Settings


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
