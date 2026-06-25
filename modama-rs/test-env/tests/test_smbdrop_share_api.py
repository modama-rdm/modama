import time

import requests

BASE_URL = "http://smbdrop:5051"
API_PATH = "/share/"


def wait_for_api(timeout: int = 60, interval: float = 1.0) -> None:
    deadline = time.time() + timeout
    while time.time() < deadline:
        try:
            response = requests.get(f"{BASE_URL}{API_PATH}", timeout=5)
            if response.status_code == 200:
                return
        except requests.RequestException:
            pass
        time.sleep(interval)
    raise RuntimeError(f"smbdrop API did not become available under {BASE_URL}{API_PATH} within {timeout}s")


def test_create_share_for_testresource() -> None:
    wait_for_api()

    payload = {
        "dest_path": "/data/adhoc/testproject/session1",
        "uid": 1001,
        "gid": 1001,
        "resource": "testresource",
    }

    create_response = requests.post(f"{BASE_URL}{API_PATH}", json=payload, timeout=30)
    assert create_response.status_code == 200, create_response.text

    body = create_response.json()
    assert body["status"] == "ok"
    share = body["share"]
    assert share["resource"] == "testresource"
    assert isinstance(share["share_id"], str) and share["share_id"]
    assert isinstance(share["share_name"], str) and share["share_name"]

    list_response = requests.get(f"{BASE_URL}{API_PATH}", timeout=30)
    assert list_response.status_code == 200, list_response.text

    shares = [
        item
        for (key, item) in list_response.json().items()
        if item.get("resource") == "testresource"
    ]
    assert shares, "expected at least one share for resource 'testresource'"
