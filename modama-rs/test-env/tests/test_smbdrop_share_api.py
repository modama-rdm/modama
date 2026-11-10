import requests


def test_create_share_for_testresource(smbdrop_api: str) -> None:
    payload = {
        "canonical_path": "adhoc/testproject/session1",
        "uid": 1001,
        "gid": 1001,
        "resource": "testresource",
    }

    create_response = requests.post(f"{smbdrop_api}/share/", json=payload, timeout=35.0)
    assert create_response.status_code == 200, create_response.text

    body = create_response.json()
    assert body["status"] == "ok"
    share = body["share"]
    assert share["resource"] == "testresource"
    assert share["canonical_path"] == "adhoc/testproject/session1"
    assert share["dest_path"] == "/data/adhoc/testproject/session1"
    assert isinstance(share["share_id"], str) and share["share_id"]
    assert isinstance(share["share_name"], str) and share["share_name"]

    list_response = requests.get(f"{smbdrop_api}/share/", timeout=5.0)
    assert list_response.status_code == 200, list_response.text

    shares = [
        item
        for (key, item) in list_response.json().items()
        if item.get("resource") == "testresource"
    ]
    assert shares, "expected at least one share for resource 'testresource'"


def test_create_share_with_dest_path_from_older_callers(smbdrop_api: str) -> None:
    # without a resource, so no agent mounts it
    payload = {
        "dest_path": "/data/adhoc/testproject/session3/",
        "uid": 1001,
        "gid": 1001,
        "resource": None,
    }

    response = requests.post(f"{smbdrop_api}/share/", json=payload, timeout=35.0)
    assert response.status_code == 200, response.text
    share = response.json()["share"]
    assert share["canonical_path"] == "adhoc/testproject/session3"
    assert share["dest_path"] == "/data/adhoc/testproject/session3"


def test_create_share_refuses_paths_outside_of_the_data_root(smbdrop_api: str) -> None:
    for paths in [
        {"canonical_path": "/data/adhoc/testproject/session1"},
        {"canonical_path": "adhoc/../../etc"},
        {"dest_path": "/etc"},
        {
            "canonical_path": "adhoc/testproject/session1",
            "dest_path": "/data/adhoc/testproject/session1",
        },
    ]:
        payload = {**paths, "uid": 1001, "gid": 1001, "resource": None}
        response = requests.post(f"{smbdrop_api}/share/", json=payload, timeout=35.0)
        assert response.status_code == 400, (paths, response.text)
