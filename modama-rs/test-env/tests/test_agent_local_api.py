import requests


def test_agent_api_connectivity(agent_api: str) -> None:
    response = requests.get(agent_api, timeout=3.0)
    assert response.status_code in (200, 401), response.text


def test_agent_api_create_share(agent_api: str):
    response = requests.post(
        url=f"{agent_api}/start-ingest/testproject/session2/",
        auth=requests.auth.HTTPBasicAuth("clausen", "1234"),
        timeout=30,
    )
    response.raise_for_status()
