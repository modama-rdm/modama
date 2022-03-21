from typing import Optional

import requests

from permissions_client import PermissionsClient
from smbdrop_client import ShareClient


class MgmtClient(PermissionsClient, ShareClient):
    def __init__(self, base_url: str,  username: Optional[str] = None, password: Optional[str] = None):
        PermissionsClient.__init__(
            base_url=base_url,
            username=username,
            password=password,
        )
        ShareClient.__init__(base_url=base_url)

    @property
    def auth(self):
        return requests.auth.HTTPBasicAuth(self.username, self.password)
