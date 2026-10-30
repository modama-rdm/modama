import time

import requests


def wait_for_session(name: str, agent_api: str):
    print(f"waiting for session {name}")
    deadline = time.time() + 30
    while time.time() < deadline:
        response = requests.get(
            url=f"{agent_api}/api/shares/foreground/",
            auth=requests.auth.HTTPBasicAuth("clausen", "1234"),
            headers={"X-API-Version": "1.0.0"},
            timeout=30,
        )
        response.raise_for_status()
        fg = response.json()
        print(fg)
        if fg["type"] == "Mounted" and fg["session"] == name:
            break
        time.sleep(0.3)
    else:
        raise ValueError(f"foreground share for {name} did not mount")


def test_agent_api_connectivity(agent_api: str) -> None:
    response = requests.get(agent_api, timeout=3.0)
    assert response.status_code in (200, 401), response.text


def test_agent_api_start_ingest(agent_api: str):
    response = requests.post(
        url=f"{agent_api}/api/projects/testproject/sessions/session2/start-ingest/",
        auth=requests.auth.HTTPBasicAuth("clausen", "1234"),
        headers={"X-API-Version": "1.0.0"},
        timeout=35,
    )
    response.raise_for_status()
    share = response.json()
    assert share["dest_path"] == "/data/adhoc/testproject/session2"
    assert share["project"] == "testproject"
    assert share["session"] == "session2"
    assert "share_id" in share

    # foreground share eventually switches to session2:
    wait_for_session("session2", agent_api)


def test_agent_api_start_multiple_ingests(agent_api: str):
    time.sleep(
        1.1
    )  # session creation is recorded in second resolution, so wait a bit to ensure we become the fg share
    response = requests.post(
        url=f"{agent_api}/api/projects/testproject/sessions/session6/start-ingest/",
        auth=requests.auth.HTTPBasicAuth("clausen", "1234"),
        headers={"X-API-Version": "1.0.0"},
        timeout=35,
    )
    response.raise_for_status()
    share = response.json()
    assert share["dest_path"] == "/data/adhoc/testproject/session6"
    assert share["project"] == "testproject"
    assert share["session"] == "session6"
    assert "share_id" in share

    # foreground share eventually switches to session6:
    wait_for_session("session6", agent_api)
    time.sleep(
        1.1
    )  # session creation is recorded in second resolution, so wait a bit to ensure we become the fg share

    response = requests.post(
        url=f"{agent_api}/api/projects/testproject/sessions/session7/start-ingest/",
        auth=requests.auth.HTTPBasicAuth("clausen", "1234"),
        headers={"X-API-Version": "1.0.0"},
        timeout=30,
    )
    response.raise_for_status()
    share = response.json()
    assert share["dest_path"] == "/data/adhoc/testproject/session7"
    assert share["project"] == "testproject"
    assert share["session"] == "session7"
    assert "share_id" in share

    # foreground share eventually switches to session7:
    wait_for_session("session7", agent_api)
    # FIXME: assert: eventually, both session1 and session2 are mounted on the agents
    # FIXME: clean up


def test_agent_api_create_session(agent_api: str):
    response = requests.post(
        url=f"{agent_api}/api/projects/testproject/sessions/",
        auth=requests.auth.HTTPBasicAuth("clausen", "1234"),
        headers={"X-API-Version": "1.0.0"},
        json={
            "session": "2028-01-01-brand-new-session",
        },
        timeout=3,
    )
    response.raise_for_status()
    resp_data = response.json()
    assert resp_data == {
        "project": "testproject",
        "session": "2028-01-01-brand-new-session",
    }

    response = requests.get(
        url=f"{agent_api}/api/projects/testproject/sessions/",
        headers={"X-API-Version": "1.0.0"},
        auth=requests.auth.HTTPBasicAuth("clausen", "1234"),
        timeout=3,
    )
    response.raise_for_status()
    resp_data = response.json()
    assert resp_data["project"] == "testproject"
    sessions = resp_data["sessions"]
    assert "2028-01-01-brand-new-session" in sessions


def test_agent_api_list_sessions(agent_api: str):
    response = requests.get(
        url=f"{agent_api}/api/projects/testproject/sessions/",
        headers={"X-API-Version": "1.0.0"},
        auth=requests.auth.HTTPBasicAuth("clausen", "1234"),
        timeout=3,
    )
    response.raise_for_status()
    resp_data = response.json()
    assert resp_data["project"] == "testproject"
    sessions = resp_data["sessions"]
    for s in ["session1", "session2", "session3", "session4"]:
        assert s in sessions


def test_agent_api_list_projects(agent_api: str):
    response = requests.get(
        url=f"{agent_api}/api/projects/",
        headers={"X-API-Version": "1.0.0"},
        auth=requests.auth.HTTPBasicAuth("clausen", "1234"),
        timeout=3,
    )
    response.raise_for_status()
    resp_data = response.json()
    projects = resp_data["projects"]
    p0 = projects[0]
    assert p0["name"] == "testproject"
    assert p0["owner"] == "clausen"
    assert "members" in p0
    assert "modified_at" in p0


def test_create_duplicate_session(agent_api: str):
    name = "duplicate-session"
    response = requests.post(
        url=f"{agent_api}/api/projects/testproject/sessions/",
        headers={"X-API-Version": "1.0.0"},
        auth=requests.auth.HTTPBasicAuth("clausen", "1234"),
        json={
            "session": name,
        },
        timeout=3,
    )
    response.raise_for_status()

    response = requests.post(
        url=f"{agent_api}/api/projects/testproject/sessions/",
        headers={"X-API-Version": "1.0.0"},
        auth=requests.auth.HTTPBasicAuth("clausen", "1234"),
        json={
            "session": name,
        },
        timeout=3,
    )
    assert (
        response.status_code == 500
    )  # should be bad request - the session already exists
