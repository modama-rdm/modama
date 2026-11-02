import requests

from smbdrop_client.schema import (
    CreateShareResponse,
    DeleteShareResponse,
    ListSharesResponse,
)


def _make_url(base_url: str, path: str) -> str:
    assert base_url[-1] != "/"
    return base_url + path


class ShareClient:
    def __init__(self, base_url: str):
        self._base_url = base_url

    @property
    def auth(self):
        return None

    def create(
        self,
        dest_path: str,
        uid: int,
        gid: int,
        resource: str | None = None,
        created_by: str | None = None,
    ) -> CreateShareResponse:
        url = _make_url(self._base_url, "/share/")
        payload = {
            "dest_path": dest_path,
            "uid": uid,
            "gid": gid,
            "resource": resource,
            "created_by": created_by,
        }
        resp = requests.post(url, json=payload, auth=self.auth)
        if not resp.ok:
            raise RuntimeError(
                f"something went wrong when creating the share for {dest_path},"
                " please check the logs!"
            )
        resp_data: CreateShareResponse = resp.json()
        return resp_data

    def list(self) -> ListSharesResponse:
        url = _make_url(self._base_url, "/share/")
        resp = requests.get(url, auth=self.auth)
        if not resp.ok:
            raise RuntimeError(
                "something went wrong when listing shares, please check the logs!"
            )
        resp_data: ListSharesResponse = resp.json()
        return resp_data

    def delete(self, share_id: str) -> DeleteShareResponse:
        url = _make_url(self._base_url, f"/share/{share_id}/")
        resp = requests.delete(url, auth=self.auth)
        if not resp.ok:
            raise RuntimeError(
                f"something went wrong when deleting the share {share_id},"
                " please check the logs!"
            )
        return resp.json()
