import pytest
from conftest import api_client
from fastapi.testclient import TestClient
from mgmtinterface_api import API_PREFIX, create_app
from mgmtinterface_api.config import Settings
from mgmtinterface_api.errors import APIError


@pytest.fixture
def client(settings: Settings) -> TestClient:
    app = create_app(settings)

    @app.get(f"{API_PREFIX}/boom")
    def boom() -> None:
        raise APIError(403, "not_a_member", "You are not a member of this project.")

    return api_client(app)


def test_healthz(client: TestClient):
    response = client.get(f"{API_PREFIX}/healthz")
    assert response.status_code == 200
    assert response.json() == {"status": "ok"}


def test_api_error_uses_status_code_and_house_body(client: TestClient):
    response = client.get(f"{API_PREFIX}/boom")
    # Real status code, not HTTP 200 + `errno` like the legacy Flask API.
    assert response.status_code == 403
    assert response.json() == {
        "code": "not_a_member",
        "message": "You are not a member of this project.",
    }


def test_errors_never_challenge_with_basic_auth(client: TestClient):
    """
    A `WWW-Authenticate: Basic` header makes the browser pop its own login
    dialog on top of the SPA's login screen. The Flask API needed an explicit
    workaround for this; here it just has to never happen.
    """
    response = client.get(f"{API_PREFIX}/boom")
    assert "www-authenticate" not in {k.lower() for k in response.headers}


def test_unconfigured_session_secret_is_refused():
    """
    A deployment that forgets to mount the secret file fails visibly, instead of
    coming up on a temporary key that signs everyone out at the next restart.
    """
    unconfigured = Settings(session_secret="", session_secret_file="")
    with pytest.raises(RuntimeError, match="No session secret configured"):
        _ = unconfigured.resolve_session_secret()


def test_app_refuses_to_start_without_a_session_secret():
    with pytest.raises(RuntimeError, match="No session secret configured"):
        _ = create_app(Settings(session_secret="", session_secret_file=""))


def test_dev_mode_session_secret_is_generated_not_defaulted():
    """
    No hardcoded fallback: a predictable signing key would let anyone mint a
    session cookie. A random per-boot key is fine; it just doesn't outlive the
    process.
    """
    dev = Settings(session_secret="", session_secret_file="", dev_mode=True)
    generated = dev.resolve_session_secret()
    assert len(generated) >= 32
    # Stable within the process, or every request would invalidate the last.
    assert dev.resolve_session_secret() == generated
    # ...and not shared with any other boot.
    other = Settings(session_secret="", session_secret_file="", dev_mode=True)
    assert other.resolve_session_secret() != generated


def test_app_starts_without_session_configuration_in_dev_mode():
    app = create_app(Settings(session_secret="", session_secret_file="", dev_mode=True))
    assert api_client(app).get(f"{API_PREFIX}/healthz").status_code == 200


def test_session_secret_read_from_file(tmp_path):
    secret_file = tmp_path / "session_secret"
    _ = secret_file.write_text("s3cret\n", encoding="utf8")
    settings = Settings(session_secret_file=str(secret_file))
    assert settings.resolve_session_secret() == "s3cret"
