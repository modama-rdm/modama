import pathlib

import requests


def test_basic_token_api_broken_token(smbdrop_token_api: str, ca_cert: pathlib.Path):
    resp = requests.put(smbdrop_token_api + "/verify/", data="thisisabrokentoken", verify=str(ca_cert))
    resp.raise_for_status()
    resp_data = resp.json()
    assert resp_data == "Invalid"
