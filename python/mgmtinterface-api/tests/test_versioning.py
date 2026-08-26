"""
The version contract itself: what the server does when a client says which
contract it was built against, and — the point of the whole exercise — what it
does when a client says nothing.
"""

from datetime import datetime, timezone

import pytest
from conftest import api_client
from fastapi.testclient import TestClient
from mgmtinterface_api import API_PREFIX, create_app
from mgmtinterface_api.config import Settings
from mgmtinterface_api.versioning import (
    API_VERSION_HEADER,
    LATEST_VERSION,
    SUPPORTED_HEADER,
    SUPPORTED_VERSIONS,
    Deprecation,
)

PROJECTS = f"{API_PREFIX}/projects"
ECHO = f"{API_PREFIX}/echo"


@pytest.fixture
def app(settings: Settings):
    """The real app plus one unauthenticated route, to observe a 2xx response."""
    built = create_app(settings)

    @built.get(ECHO)
    def echo() -> dict[str, str]:
        return {"ok": "yes"}

    return built


@pytest.fixture
def unversioned(app) -> TestClient:
    """A client that sends no version header — i.e. a caller that got it wrong."""
    return TestClient(app)


def test_request_without_version_header_is_refused(unversioned: TestClient):
    """
    The rule the whole design rests on: the server does not guess.

    A locally installed client that omits the header must fail loudly now rather
    than be silently moved onto a newer contract by some future deploy.
    """
    response = unversioned.get(PROJECTS)
    assert response.status_code == 400
    assert response.json()["code"] == "missing_api_version"


def test_missing_version_is_refused_before_authentication(unversioned: TestClient):
    """
    App-wide dependency order: the version is resolved before anything else has
    an opinion, so an unauthenticated caller is told the real problem instead of
    being sent off to sign in first.
    """
    assert unversioned.get(PROJECTS).status_code == 400


@pytest.mark.parametrize("value", ["99", "0", "v1", "", "latest", "1.0"])
def test_unknown_version_is_a_400_not_a_404(app, value: str):
    """
    400, so a lab log says *why*. A 404 would read as a wrong URL and send
    whoever is debugging it looking in the wrong place. `v1` is rejected on
    purpose: one accepted spelling means a client cannot half-work.
    """
    client = TestClient(app, headers={API_VERSION_HEADER: value})
    response = client.get(PROJECTS)
    assert response.status_code == 400
    assert response.json()["code"] == "unsupported_api_version"


def test_errors_advertise_what_the_server_does_support(unversioned: TestClient):
    """The client that got it wrong is the one that needs the list."""
    response = unversioned.get(PROJECTS)
    assert response.headers[SUPPORTED_HEADER] == ", ".join(
        str(v) for v in SUPPORTED_VERSIONS
    )


def test_resolved_version_is_echoed_and_varied_on(app):
    response = api_client(app).get(ECHO)
    assert response.status_code == 200
    assert response.headers[API_VERSION_HEADER] == str(LATEST_VERSION)
    # Without Vary, a shared cache could hand one version's body to a client
    # that asked for another.
    vary = {part.strip().lower() for part in response.headers["vary"].split(",")}
    assert API_VERSION_HEADER.lower() in vary


def test_surrounding_whitespace_is_tolerated(app):
    client = TestClient(app, headers={API_VERSION_HEADER: f" {LATEST_VERSION} "})
    assert client.get(ECHO).status_code == 200


def test_healthz_answers_without_a_version(unversioned: TestClient):
    """The container probe predates any notion of a contract."""
    assert unversioned.get(f"{API_PREFIX}/healthz").status_code == 200


def test_versions_endpoint_answers_without_a_version(unversioned: TestClient):
    """
    A client calls this precisely because it does not yet know what exists, so
    requiring a version here would be a chicken-and-egg.
    """
    response = unversioned.get(f"{API_PREFIX}/versions")
    assert response.status_code == 200
    assert response.json() == {
        "supported": list(SUPPORTED_VERSIONS),
        "latest": LATEST_VERSION,
        "deprecated": [],
    }


def test_deprecated_version_advertises_its_sunset(app, monkeypatch: pytest.MonkeyPatch):
    """
    A client on a doomed version learns it from every response, months before
    the version is switched off — that warning is what makes a slow local
    update cycle survivable.
    """
    from mgmtinterface_api import versioning

    since = datetime(2027, 1, 4, tzinfo=timezone.utc)
    monkeypatch.setattr(
        versioning,
        "DEPRECATIONS",
        {
            LATEST_VERSION: Deprecation(
                since=since, sunset=datetime(2028, 1, 4, tzinfo=timezone.utc)
            )
        },
    )

    response = api_client(app).get(ECHO)
    # RFC 9745: a structured-field date, seconds since the epoch after '@'.
    assert response.headers["deprecation"] == f"@{int(since.timestamp())}"
    # RFC 8594: a plain HTTP-date.
    assert response.headers["sunset"] == "Tue, 04 Jan 2028 00:00:00 GMT"


def test_supported_version_is_not_deprecated_by_default(app):
    response = api_client(app).get(ECHO)
    assert "deprecation" not in {k.lower() for k in response.headers}


def test_clients_are_logged_once_per_boot(app, caplog: pytest.LogCaptureFixture):
    """
    Retiring a version is an observation, not a calendar promise — the logs have
    to be able to answer "is anything still on version 1". One line per client,
    not one per request, or the answer drowns.
    """
    client = api_client(app, headers={"user-agent": "modama-agent/1.2.3"})
    with caplog.at_level("INFO", logger="mgmtinterface_api.versioning"):
        client.get(ECHO)
        client.get(ECHO)

    lines = [r for r in caplog.records if "First request this boot" in r.message]
    assert len(lines) == 1
    assert "modama-agent/1.2.3" in lines[0].getMessage()


@pytest.mark.parametrize("spelling", ["+1", "01", "1_0", "1.0", "v1", "1 1", ""])
def test_only_one_spelling_of_a_version_is_accepted(app, spelling: str):
    """
    Anything but a bare integer is an unsupported version, not a value to
    interpret.

    `int()` on its own would read most of these as 1 or 10 — and a client that
    half-works on a form we happen to tolerate today is exactly what breaks when
    the parsing is later tightened. Rejecting them now costs nothing: no client
    can already depend on a spelling the server never accepted.
    """
    client = TestClient(app, headers={API_VERSION_HEADER: spelling})
    response = client.get(ECHO)
    assert response.status_code == 400
    assert response.json()["code"] == "unsupported_api_version"


def test_the_supported_version_is_still_accepted_plainly(app):
    """The guard above must not have narrowed the one spelling that counts."""
    client = TestClient(app, headers={API_VERSION_HEADER: str(LATEST_VERSION)})
    assert client.get(ECHO).status_code == 200
