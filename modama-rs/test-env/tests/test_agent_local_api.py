import requests


def test_agent_api_connectivity(agent_api: str) -> None:
    response = requests.get(agent_api, timeout=3.0)
    assert response.status_code in (200, 401), response.text


def test_agent_api_create_share(agent_api: str):
    response = requests.post(
        url=f"{agent_api}/api/projects/testproject/sessions/session2/start-ingest/",
        auth=requests.auth.HTTPBasicAuth("clausen", "1234"),
        timeout=30,
    )
    response.raise_for_status()
    resp_data = response.json()
    share = resp_data["share"]
    assert share["resource"] == "testresource"
    # FIXME: this should change to an /adhoc/-relative path:
    assert share["dest_path"] == "/data/adhoc/testproject/session2"
    assert share["uid"] == 1001
    assert share["gid"] == 1002


def test_agent_api_create_session(agent_api: str):
    response = requests.post(
        url=f"{agent_api}/api/projects/testproject/sessions/",
        auth=requests.auth.HTTPBasicAuth("clausen", "1234"),
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
        auth=requests.auth.HTTPBasicAuth("clausen", "1234"),
        timeout=3,
    )
    response.raise_for_status()
    resp_data = response.json()
    assert resp_data == {
        "projects": ["testproject"],
    }
