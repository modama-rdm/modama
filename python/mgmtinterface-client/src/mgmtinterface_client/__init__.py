import requests

from permissions_client import PermissionsClient, quote
from smbdrop_client.schema import (
    DeleteShareResponse,
    ListSharesResponse,
    SerializedShare,
)


class MgmtClient(PermissionsClient):
    def __init__(
        self,
        base_url: str,
        username: str | None = None,
        password: str | None = None,
    ):
        PermissionsClient.__init__(
            self,
            base_url=base_url,
            username=username,
            password=password,
        )

    @property
    def auth(self):
        return requests.auth.HTTPBasicAuth(self.username, self.password)

    def create_share(
        self, project: str, dataset: str, resource: str | None = None
    ) -> SerializedShare:
        """ """
        resp = requests.post(
            f"{self.base_url}/share/",
            json={
                "project": project,
                "dataset": dataset,
                "resource": resource,
            },
            auth=self.auth,
        )
        resp.raise_for_status()
        resp_data = resp.json()
        return resp_data

    def list_shares(self) -> ListSharesResponse:
        resp = requests.get(f"{self.base_url}/share/", auth=self.auth)
        resp.raise_for_status()
        resp_data = resp.json()
        return resp_data

    def delete_share(self, share_id: str) -> DeleteShareResponse:
        resp = requests.delete(
            f"{self.base_url}/share/{quote(share_id)}/", auth=self.auth
        )
        resp.raise_for_status()
        resp_data = resp.json()
        return resp_data
