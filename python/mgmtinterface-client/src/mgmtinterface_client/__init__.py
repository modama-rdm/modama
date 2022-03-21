from typing import Optional, Dict

import requests

from permissions_client import PermissionsClient, quote


class MgmtClient(PermissionsClient):
    def __init__(self, base_url: str,  username: Optional[str] = None, password: Optional[str] = None):
        PermissionsClient.__init__(
            base_url=base_url,
            username=username,
            password=password,
        )

    @property
    def auth(self):
        return requests.auth.HTTPBasicAuth(self.username, self.password)

    def create_share(self, project: str, dataset: str) -> Dict:
        """
        """
        resp = requests.post(
            f"{self.base_url}/share/",
            json={
                'project': project,
                'dataset': dataset,
            },
            auth=self.auth
        )
        resp.raise_for_status()
        resp_data: Dict = resp.json()
        return resp_data

    def list_shares(self) -> Dict:
        resp = requests.get(
            f"{self.base_url}/share/",
            auth=self.auth
        )
        resp.raise_for_status()
        resp_data: Dict = resp.json()
        return resp_data

    def delete_share(self, share_id: int) -> Dict:
        resp = requests.delete(
            f"{self.base_url}/share/{quote(share_id)}/",
            auth=self.auth
        )
        resp.raise_for_status()
        resp_data: Dict = resp.json()
        return resp_data
