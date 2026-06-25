import time

import requests

AGENT_BASE_URL = "http://modama-agent-1:3003"
AGENT_API_PATH = "/"


def wait_for_agent_api(timeout: int = 60, interval: float = 1.0) -> None:
    deadline = time.time() + timeout
    while time.time() < deadline:
        try:
            response = requests.get(f"{AGENT_BASE_URL}{AGENT_API_PATH}", timeout=5)
            if response.status_code in (200, 401):
                return
        except requests.RequestException:
            pass
        time.sleep(interval)
    raise RuntimeError(
        f"modama-agent-1 local API did not become available under {AGENT_BASE_URL}{AGENT_API_PATH} within {timeout}s"
    )


def test_agent_api_connectivity() -> None:
    wait_for_agent_api()

    response = requests.get(f"{AGENT_BASE_URL}{AGENT_API_PATH}", timeout=30)
    assert response.status_code in (200, 401), response.text
