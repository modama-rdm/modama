import time
from http.cookies import SimpleCookie

import pytest
from conftest import api_client
from fastapi.testclient import TestClient
from mgmtinterface_api import API_PREFIX, create_app
from mgmtinterface_api.config import Settings
from mgmtinterface_api.routes import auth as auth_routes

LOGIN = f"{API_PREFIX}/auth/login"
LOGOUT = f"{API_PREFIX}/auth/logout"
SESSION = f"{API_PREFIX}/auth/session"

GOOD = {"username": "mmustermann", "password": "correct horse"}


@pytest.fixture
def upstream(monkeypatch: pytest.MonkeyPatch) -> dict[str, int]:
    """
    Replaces the permissions service and counts the calls, so tests can check
    that re-validation happens on refresh and not on every request.
    """
    calls = {"authenticate": 0, "user_exists": 0}

    def fake_authenticate(settings: Settings, username: str, password: str) -> bool:
        calls["authenticate"] += 1
        return username == GOOD["username"] and password == GOOD["password"]

    def fake_user_exists(settings: Settings, username: str) -> bool:
        calls["user_exists"] += 1
        return username == GOOD["username"]

    monkeypatch.setattr(auth_routes, "authenticate", fake_authenticate)
    monkeypatch.setattr("mgmtinterface_api.security.user_exists", fake_user_exists)
    return calls


@pytest.fixture
def client(settings: Settings, upstream: dict[str, int]) -> TestClient:
    return api_client(create_app(settings))


def test_login_sets_a_session_cookie(client: TestClient, settings: Settings):
    response = client.post(LOGIN, json=GOOD)
    assert response.status_code == 200
    assert response.json() == {"username": "mmustermann"}

    cookie = response.cookies.get(settings.effective_cookie_name)
    assert cookie is not None
    set_cookie = response.headers["set-cookie"].lower()
    assert "httponly" in set_cookie
    assert "samesite=lax" in set_cookie
    assert "path=/" in set_cookie


def test_login_response_is_not_cacheable(client: TestClient):
    assert client.post(LOGIN, json=GOOD).headers["cache-control"] == "no-store"


def test_cookie_name_does_not_collide_with_flask_login(settings: Settings):
    # Both apps are live on the same origin until the cutover (#170).
    assert settings.effective_cookie_name != "mgmtinterface_session"


def test_secure_deployments_use_the_host_cookie_prefix():
    """
    The browser rejects a `__Host-` cookie unless it is Secure, has Path=/ and
    no Domain. This prevents another host under fz-juelich.de from setting a
    session cookie for this service.
    """
    secure = Settings(session_secret="x", session_cookie_secure=True)
    assert secure.effective_cookie_name == "__Host-modama_session"
    # The prefix requires Secure, so plain-http development cannot use it.
    insecure = Settings(session_secret="x", session_cookie_secure=False)
    assert insecure.effective_cookie_name == "modama_session"


def test_host_prefixed_cookie_carries_the_attributes_the_prefix_requires(
    upstream: dict[str, int],
):
    app = create_app(Settings(session_secret="x", session_cookie_secure=True))
    client = api_client(app, base_url="https://testserver")
    set_cookie = client.post(LOGIN, json=GOOD).headers["set-cookie"]
    assert set_cookie.startswith("__Host-modama_session=")
    assert "Secure" in set_cookie
    assert "Path=/" in set_cookie
    assert "Domain" not in set_cookie


def test_login_rejects_bad_credentials(client: TestClient, settings: Settings):
    response = client.post(LOGIN, json={**GOOD, "password": "wrong"})
    assert response.status_code == 401
    assert response.json()["code"] == "invalid_credentials"
    assert settings.effective_cookie_name not in response.cookies
    # Must not trigger the browser's own login dialog.
    assert "www-authenticate" not in {k.lower() for k in response.headers}


def test_login_validates_its_body(client: TestClient):
    response = client.post(LOGIN, json={"username": "", "password": ""})
    # FastAPI's own 422 shape is the documented exception to the error body.
    assert response.status_code == 422


def test_session_requires_a_cookie(client: TestClient):
    response = client.get(SESSION)
    assert response.status_code == 401
    assert response.json()["code"] == "not_authenticated"


def test_session_returns_the_signed_in_user(client: TestClient):
    _ = client.post(LOGIN, json=GOOD)
    response = client.get(SESSION)
    assert response.status_code == 200
    assert response.json() == {"username": "mmustermann"}


def test_logout_clears_the_cookie(client: TestClient):
    _ = client.post(LOGIN, json=GOOD)
    response = client.post(LOGOUT)
    assert response.status_code == 204
    assert client.get(SESSION).status_code == 401


def test_logout_without_a_session_is_not_an_error(client: TestClient):
    assert client.post(LOGOUT).status_code == 204


def test_forged_cookie_is_rejected(client: TestClient, settings: Settings):
    other = create_app(Settings(session_secret="other", session_cookie_secure=False))
    forged = other.state.session_codec.issue("mmustermann")
    client.cookies.set(settings.effective_cookie_name, forged)
    response = client.get(SESSION)
    assert response.status_code == 401
    assert response.json()["code"] == "session_expired"


def expired_cookie_directives(response) -> str:
    """The `Set-Cookie` that deletes the session, or "" if there is none."""
    for value in response.headers.get_list("set-cookie"):
        # A deletion is an empty value plus an expiry in the past.
        if "Max-Age=0" in value or "Expires=Thu, 01 Jan 1970" in value:
            return value
    return ""


def test_a_dead_session_is_cleared_from_the_browser(
    client: TestClient, settings: Settings
):
    """
    The 401 has to carry the cookie deletion.

    The error is raised and the exception handler builds its own response, so
    a `Set-Cookie` written to the handler's injected `Response` would be
    dropped, and the browser would keep sending a cookie that is never
    accepted.
    """
    other = create_app(Settings(session_secret="other", session_cookie_secure=False))
    client.cookies.set(
        settings.effective_cookie_name, other.state.session_codec.issue("mmustermann")
    )

    response = client.get(SESSION)

    assert response.status_code == 401
    deletion = expired_cookie_directives(response)
    assert deletion, "the dead session cookie was not cleared"
    assert settings.effective_cookie_name in deletion
    # Attributes have to match the ones it was set with, or the browser keeps
    # the original cookie alongside the deletion.
    assert "Path=/" in deletion
    assert "HttpOnly" in deletion


def test_a_session_whose_account_vanished_is_also_cleared(
    settings: Settings, upstream: dict[str, int], monkeypatch: pytest.MonkeyPatch
):
    """The other 401 path out of `current_session`, with the same requirement."""
    monkeypatch.setattr(
        "mgmtinterface_api.security.user_exists", lambda settings, username: False
    )
    app = create_app(always_refreshing(settings))
    client = api_client(app)
    client.cookies.set(
        settings.effective_cookie_name, app.state.session_codec.issue("mmustermann")
    )

    response = client.get(SESSION)

    assert response.status_code == 401
    assert expired_cookie_directives(response), "the cookie was not cleared"


def test_fresh_session_does_not_call_the_permissions_service(
    client: TestClient, upstream: dict[str, int]
):
    """
    A cookie that was just issued must not cause a call to the permissions
    service. Otherwise this would be the same as Basic auth, with one upstream
    call per request.
    """
    _ = client.post(LOGIN, json=GOOD)
    for _i in range(3):
        assert client.get(SESSION).status_code == 200
    assert upstream["user_exists"] == 0


def always_refreshing(settings: Settings) -> Settings:
    """Refresh interval of 0, so every request takes the refresh path."""
    return Settings(**{**settings.model_dump(), "session_refresh_seconds": 0})


def test_stale_cookie_is_refreshed_and_revalidated(
    settings: Settings, upstream: dict[str, int]
):
    app = create_app(always_refreshing(settings))
    client = api_client(app)
    issued = int(time.time()) - 60
    client.cookies.set(
        settings.effective_cookie_name,
        app.state.session_codec.issue("mmustermann", issued=issued),
    )

    response = client.get(SESSION)
    assert response.status_code == 200
    assert upstream["user_exists"] == 1

    # A refreshed cookie comes back on the response and carries the original
    # `issued`, so refreshing cannot extend the absolute lifetime.
    jar = SimpleCookie(response.headers["set-cookie"])
    refreshed, _ = app.state.session_codec.read(
        jar[settings.effective_cookie_name].value
    )
    assert refreshed["issued"] == issued


def test_session_ends_when_the_account_disappears(
    settings: Settings, upstream: dict[str, int], monkeypatch: pytest.MonkeyPatch
):
    monkeypatch.setattr(
        "mgmtinterface_api.security.user_exists", lambda settings, username: False
    )
    app = create_app(always_refreshing(settings))
    client = api_client(app)
    client.cookies.set(
        settings.effective_cookie_name, app.state.session_codec.issue("mmustermann")
    )
    response = client.get(SESSION)
    assert response.status_code == 401
    assert response.json()["code"] == "session_expired"


def test_cross_origin_post_is_rejected(client: TestClient):
    response = client.post(LOGIN, json=GOOD, headers={"Origin": "https://evil.example"})
    assert response.status_code == 403
    assert response.json()["code"] == "bad_origin"


def test_same_origin_post_is_allowed(client: TestClient):
    response = client.post(
        LOGIN, json=GOOD, headers={"Origin": "http://testserver", "Host": "testserver"}
    )
    assert response.status_code == 200


def test_origin_is_compared_against_the_forwarded_scheme(client: TestClient):
    """
    Behind nginx the request arrives over http while the browser saw https, so
    the check has to use X-Forwarded-Proto or every real POST would 403.
    """
    response = client.post(
        LOGIN,
        json=GOOD,
        headers={
            "Origin": "https://er-c-data-mgmt.fz-juelich.de",
            "Host": "er-c-data-mgmt.fz-juelich.de",
            "X-Forwarded-Proto": "https",
        },
    )
    assert response.status_code == 200


def test_origin_keeps_a_non_default_port(client: TestClient):
    """
    The test deployment answers on :5001, so the browser's Origin carries that
    port. nginx must forward it ($http_host, not $host), otherwise every POST
    gets a 403. This once showed up as a seemingly wrong password.
    """
    response = client.post(
        LOGIN,
        json=GOOD,
        headers={"Origin": "http://localhost:5001", "Host": "localhost:5001"},
    )
    assert response.status_code == 200


def test_explicit_default_port_still_matches(client: TestClient):
    # The opposite case: a Host header with an explicit :443 must still match
    # the Origin, which the browser sends without the port for https.
    response = client.post(
        LOGIN,
        json=GOOD,
        headers={
            "Origin": "https://er-c-data-mgmt.fz-juelich.de",
            "Host": "er-c-data-mgmt.fz-juelich.de:443",
            "X-Forwarded-Proto": "https",
        },
    )
    assert response.status_code == 200


def test_a_different_port_is_still_cross_origin(client: TestClient):
    response = client.post(
        LOGIN,
        json=GOOD,
        headers={"Origin": "http://localhost:9999", "Host": "localhost:5001"},
    )
    assert response.status_code == 403
    assert response.json()["code"] == "bad_origin"


def test_missing_origin_is_allowed(client: TestClient):
    # curl, scripts and the test suite send no Origin; browsers always do on
    # cross-site state-changing requests.
    assert client.post(LOGIN, json=GOOD).status_code == 200


def test_configured_extra_origin_is_accepted(
    settings: Settings, upstream: dict[str, int]
):
    """
    The vite dev server proxies /api with `changeOrigin`, so the Host stops
    matching the browser's Origin and every POST would 403 in local development.
    """
    app = create_app(
        Settings(
            **{**settings.model_dump(), "csrf_extra_origins": "http://localhost:5180"}
        )
    )
    response = api_client(app).post(
        LOGIN, json=GOOD, headers={"Origin": "http://localhost:5180"}
    )
    assert response.status_code == 200


def test_extra_origins_do_not_open_the_door_to_anything_else(
    settings: Settings, upstream: dict[str, int]
):
    app = create_app(
        Settings(
            **{**settings.model_dump(), "csrf_extra_origins": "http://localhost:5180"}
        )
    )
    response = api_client(app).post(
        LOGIN, json=GOOD, headers={"Origin": "https://evil.example"}
    )
    assert response.status_code == 403


def test_referer_is_checked_when_origin_is_absent(client: TestClient):
    # OWASP's fallback order: Origin, then Referer.
    response = client.post(
        LOGIN, json=GOOD, headers={"Referer": "https://evil.example/attack.html"}
    )
    assert response.status_code == 403
    assert response.json()["code"] == "bad_origin"


def test_same_origin_referer_is_allowed(client: TestClient):
    response = client.post(
        LOGIN, json=GOOD, headers={"Referer": "http://testserver/new/login"}
    )
    assert response.status_code == 200


def test_repeated_failures_are_throttled(settings: Settings, upstream: dict[str, int]):
    app = create_app(Settings(**{**settings.model_dump(), "login_max_failures": 3}))
    client = api_client(app)
    bad = {**GOOD, "password": "wrong"}

    for _i in range(3):
        assert client.post(LOGIN, json=bad).status_code == 401

    blocked = client.post(LOGIN, json=bad)
    assert blocked.status_code == 429
    assert blocked.json()["code"] == "too_many_attempts"
    assert int(blocked.headers["retry-after"]) > 0
    # Blocked attempts must not reach the permissions service either.
    assert upstream["authenticate"] == 3

    # Even the correct password has to wait for the window to pass. Otherwise
    # the throttle could be bypassed by guessing.
    assert client.post(LOGIN, json=GOOD).status_code == 429


def test_throttle_counts_only_failures(settings: Settings, upstream: dict[str, int]):
    app = create_app(Settings(**{**settings.model_dump(), "login_max_failures": 3}))
    client = api_client(app)
    bad = {**GOOD, "password": "wrong"}

    # Two typos followed by a success must reset the counter.
    for _i in range(2):
        assert client.post(LOGIN, json=bad).status_code == 401
    assert client.post(LOGIN, json=GOOD).status_code == 200
    for _i in range(2):
        assert client.post(LOGIN, json=bad).status_code == 401
    assert client.post(LOGIN, json=GOOD).status_code == 200


def test_throttle_key_ignores_case_and_padding(
    settings: Settings, upstream: dict[str, int]
):
    app = create_app(Settings(**{**settings.model_dump(), "login_max_failures": 2}))
    client = api_client(app)

    assert client.post(LOGIN, json={**GOOD, "password": "wrong"}).status_code == 401
    assert (
        client.post(
            LOGIN, json={"username": "MMustermann ", "password": "wrong"}
        ).status_code
        == 401
    )
    # Varying the case must not give an attacker a separate counter.
    assert client.post(LOGIN, json={**GOOD, "password": "wrong"}).status_code == 429
