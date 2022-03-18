from typing import Dict
import requests


def _make_url(base_url: str, path: str) -> str:
    assert base_url[-1] != '/'
    return base_url + path


class ShareClient:
    def __init__(self, base_url: str):
        self._base_url = base_url

    def create(self, dest_path: str, uid: int, gid: int) -> Dict:
        """
        """
        url = _make_url(self._base_url, "/share/")
        payload = {"dest_path": dest_path, "uid": uid, "gid": gid}
        resp = requests.post(url, json=payload)
        if not resp.ok:
            raise RuntimeError(
                "something went wrong when creating the share for {dest_path},"
                " please check the logs!"
            )
        resp_data: Dict = resp.json()
        return resp_data

    def list(self) -> Dict:
        url = _make_url(self._base_url, "/share/")
        resp = requests.get(url)
        if not resp.ok:
            raise RuntimeError(
                "something went wrong when listing shares,"
                " please check the logs!"
            )
        resp_data: Dict = resp.json()
        return resp_data

    def delete(self, share_id: int) -> None:
        url = _make_url(self._base_url, f"/share/{share_id}")
        resp = requests.delete(url)
        if not resp.ok:
            raise RuntimeError(
                f"something went wrong when deleting the share {share_id},"
                " please check the logs!"
            )
        return resp
