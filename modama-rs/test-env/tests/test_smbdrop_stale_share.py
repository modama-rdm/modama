import pathlib
import shutil
import subprocess
import tempfile
from typing import Any

import pytest
import requests


def create_share(
    smbdrop_api: str, dest_path: pathlib.Path, resource: str
) -> dict[str, Any]:
    response = requests.post(
        f"{smbdrop_api}/share/",
        json={
            "dest_path": str(dest_path),
            "uid": 1001,
            "gid": 1001,
            "resource": resource,
        },
        timeout=35.0,
    )
    assert response.status_code == 200, response.text
    body = response.json()
    assert body["status"] == "ok"
    return body["share"]


def connect_share(share: dict[str, Any]) -> subprocess.CompletedProcess[str]:
    with tempfile.NamedTemporaryFile(mode="w", encoding="utf-8") as auth_file:
        auth_file.write(f"username = {share['username']}\n")
        auth_file.write(f"password = {share['password']}\n")
        auth_file.write("domain = ER-C-INGEST\n")
        auth_file.flush()
        return subprocess.run(
            [
                "smbclient",
                f"//smbdrop/{share['share_name']}",
                "-A",
                auth_file.name,
                "-c",
                "ls",
            ],
            capture_output=True,
            check=False,
            text=True,
            timeout=15.0,
        )


def smb_output(result: subprocess.CompletedProcess[str]) -> str:
    return f"stdout:\n{result.stdout}\nstderr:\n{result.stderr}"


def test_stale_share_survives_source_removal(smbdrop_api: str) -> None:
    stale_path = pathlib.Path("/data/adhoc/testproject/repro-stale-share")
    healthy_path = pathlib.Path("/data/adhoc/testproject/repro-healthy-share")
    for path in (stale_path, healthy_path):
        if path.exists():
            shutil.rmtree(path)
        path.mkdir(parents=True)

    stale_share = create_share(smbdrop_api, stale_path, "repro-stale-share")
    initial_connection = connect_share(stale_share)
    assert initial_connection.returncode == 0, smb_output(initial_connection)

    shutil.rmtree(stale_path)

    # Creating another share reconciles every persisted bind mount and rewrites
    # Samba's dynamic share configuration.
    healthy_share = create_share(smbdrop_api, healthy_path, "repro-healthy-share")

    list_response = requests.get(f"{smbdrop_api}/share/", timeout=5.0)
    assert list_response.status_code == 200, list_response.text
    listed_shares = list_response.json()
    assert stale_share["share_id"] in listed_shares
    assert healthy_share["share_id"] in listed_shares

    print(
        "reproduction state: "
        f"stale_path={stale_path}, stale_share_id={stale_share['share_id']}, "
        f"stale_share_name={stale_share['share_name']}, "
        f"healthy_path={healthy_path}, healthy_share_id={healthy_share['share_id']}, "
        f"healthy_share_name={healthy_share['share_name']}"
    )

    healthy_connection = connect_share(healthy_share)
    assert healthy_connection.returncode == 0, smb_output(healthy_connection)

    stale_connection = connect_share(stale_share)
    if stale_connection.returncode == 0:
        return

    output = smb_output(stale_connection)
    if "STATUS_BAD_NETWORK_NAME" in output:
        pytest.xfail(f"reproduced lingering-share failure: {output}")
    pytest.fail(f"stale share failed for an unexpected reason: {output}")
