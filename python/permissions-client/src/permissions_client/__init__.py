from typing import Iterable, Optional
import urllib.parse

import requests
import requests.auth


def quote(s: str):
    return urllib.parse.quote(s, safe='')


class PermissionsClient:
    def __init__(
            self, base_url: str, use_auth: bool = False,
            username: Optional[str] = None, password: Optional[str] = None):
        self.base_url = base_url
        self.username = username
        self.password = password
        self.id = None

    @property
    def auth(self):
        return None

    def authenticate(self, username: str, password: str):
        r = requests.post(
            f"{self.base_url}/users/auth/", json={
                "username": str(username),
                "password": str(password),
            },
        )
        result = r.json()
        if result['errno'] == 0:
            self.username = username
            self.password = password
            self.id = result['id']
        return result

    def id_user(self, name):
        return requests.get(
            f"{self.base_url}/users/id/{quote(name)}/",
            auth=self.auth
        ).json()

    def list_users(self):
        return requests.get(
            f"{self.base_url}/users/list/",
            auth=self.auth
        ).json()

    def list_projects(self):
        return requests.get(
            f"{self.base_url}/project/",
            auth=self.auth
        ).json()

    def projectdetails(self, projectname, fast: bool = False):
        fast_qs = "?fast=1" if fast else ""
        return requests.get(
            f"{self.base_url}/project/{quote(projectname)}/{fast_qs}",
            auth=self.auth
        ).json()

    def create_project(self, name: str, owner: str, members: Iterable):
        return requests.post(
            f"{self.base_url}/project/", json={
                "name": name,
                "owner": owner,
                "members": list(members)
            },
            auth=self.auth
        ).json()

    def change_project_members(
        self, projectname: str, op, members: Iterable, user: str
    ):
        assert op in ('add', 'remove')
        return requests.patch(
            f"{self.base_url}/project/{quote(projectname)}/members/",
            json={
                "op": op,
                "members": list(members),
                "user": user
            },
            auth=self.auth
        ).json()

    def create_dataset(self, projectname: str, name: str, user: str):
        result = requests.post(
            f"{self.base_url}/project/{quote(projectname)}/dataset/",
            json={
                "name": name,
                "owner": user,
            },
            auth=self.auth
        )
        # TODO percolate consistently through client
        result.raise_for_status()
        return result.json()

    def seal_dataset(self, projectname: str, name: str, user: str):
        return requests.post(
            f"{self.base_url}/project/{quote(projectname)}/dataset/seal/",
            json={
                "name": name,
                "user": user,
            },
            auth=self.auth
        ).json()
