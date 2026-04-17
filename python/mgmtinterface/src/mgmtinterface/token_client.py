from typing import Dict, Literal, Union
from urllib.parse import urlparse, ParseResult
import requests


def _make_url(base_url: str, path: str) -> str:
    assert base_url[-1] != "/"
    return base_url + path


class VerifyFailed(Exception):
    # could be an expired token, or just plain invalid
    pass


class Agent:
    def __init__(self, url: str):
        self._url = url

    def get_resource_name(self) -> str:
        return self._parse_url().netloc

    def get_agent_name(self) -> str:
        return self._parse_url().fragment

    def _parse_url(self) -> ParseResult:
        return urlparse(self._url)

    def __repr__(self):
        return f"<Agent {self._url}>"


class TokenClient:
    def __init__(self, base_url: str, ca_cert: str):
        self._base_url = base_url
        self._ca_cert = ca_cert

    def verify_agent(self, token: str) -> Agent:
        """
        Return the verified agent identification from the given JWT,
        or raise a VerifyFailed exception.
        """
        url = _make_url(self._base_url, "/verify/")
        resp = requests.put(url, data=token.encode("utf8"), verify=self._ca_cert)
        if not resp.ok:
            raise VerifyFailed("non-200 response when verifying token")
        resp_data: Union[Dict, Literal["Invalid"]] = resp.json()
        if resp_data == "Invalid":
            raise VerifyFailed("Invalid token")
        if "VerifiedAgent" not in resp_data:
            raise VerifyFailed("Invalid token")
        return Agent(resp_data["VerifiedAgent"]["agent_id"])
