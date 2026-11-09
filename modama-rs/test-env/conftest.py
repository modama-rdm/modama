import pathlib
import time
from typing import Any

import pytest
import requests

AGENT_BASE_URL = "http://modama-agent-1:3003"
SMBDROP_BASE_URL = "http://smbdrop:5051"
SMBDROP_TOKEN_BASE_URL = "https://smbdrop:5053"


def wait_for_url(
    url: str,
    service_name: str,
    timeout: float = 60.0,
    interval: float = 1.0,
    expected_status_codes: tuple[int, ...] = (200,),
    request_kwargs: dict[Any, Any] | None = None,  # pyright: ignore[reportExplicitAny]
) -> str:
    if request_kwargs is None:
        request_kwargs = {}
    deadline = time.time() + timeout
    e = None
    while time.time() < deadline:
        try:
            response = requests.get(url, timeout=5, **request_kwargs)
            if response.status_code in expected_status_codes:
                return url
            response.raise_for_status()
        except requests.RequestException as exc:
            e = exc
            print(f"exc while wait_for_url: {e}, trying again")
        time.sleep(interval)
    raise RuntimeError(
        f"{service_name} did not become available under {url} within {timeout}s (last error: {e})"
    )


@pytest.fixture
def agent_api() -> str:
    return wait_for_url(
        AGENT_BASE_URL,
        service_name="modama-agent-1 local API",
        expected_status_codes=(200, 401),
    )


@pytest.fixture
def agent(agent_api: str) -> requests.Session:
    """
    A client of modama-agent-1 signed in as `clausen`: the agent keeps the sign-in in a
    cookie, which the session sends along
    """
    session = requests.Session()
    response = session.post(
        f"{agent_api}/api/login/",
        headers={"X-API-Version": "1.0.0"},
        json={"username": "clausen", "password": "1234"},
        timeout=10,
    )
    assert response.status_code == 200, response.text
    return session


@pytest.fixture
def smbdrop_api() -> str:
    _ = wait_for_url(SMBDROP_BASE_URL + "/health/", service_name="smbdrop API")
    return SMBDROP_BASE_URL


@pytest.fixture
def smbdrop_token_api(ca_cert: pathlib.Path) -> str:
    _ = wait_for_url(
        url=SMBDROP_TOKEN_BASE_URL + "/verify/",
        service_name="smbdrop token API",
        request_kwargs={"verify": ca_cert},
        expected_status_codes=(200, 405),  # method not allowed for GET...
    )
    return SMBDROP_TOKEN_BASE_URL


@pytest.fixture
def ca_cert() -> pathlib.Path:
    return pathlib.Path(__file__).parent / "certs" / "ca" / "pub" / "ca.cert.pem"
