from typing import Iterable
import urllib.parse

import requests


def quote(s: str):
    return urllib.parse.quote(s, safe='')


class PermissionsClient:
    def __init__(self, base_url: str):
        self.base_url = base_url

    def authenticate(self, username: str, password: str):
        r = requests.post(
            f"{self.base_url}/users/auth/", json={
                "username": str(username),
                "password": str(password),
            }
        )
        return r.json()

    def list_users(self):
        return requests.get(f"{self.base_url}/users/list/").json()

    def list_projects(self):
        return requests.get(f"{self.base_url}/project/").json()

    def projectdetails(self, projectname):
        return requests.get(f"{self.base_url}/project/{quote(projectname)}/").json()

    def create_project(self, name: str, owner: str, members: Iterable):
        return requests.post(
            f"{self.base_url}/project/", json={
                "name": name,
                "owner": owner,
                "members": list(members)
            }
        ).json()

    def change_project_members(self, projectname: str, op, members: Iterable, user: str):
        assert op in ('add', 'remove')
        return requests.patch(
            f"{self.base_url}/project/{quote(projectname)}/members/",
            json={
                "op": op,
                "members": list(members),
                "user": user
            },
        ).json()

    def create_dataset(self, projectname: str, name: str, user: str):
        return requests.post(
            f"{self.base_url}/project/{quote(projectname)}/dataset/",
            json={
                "name": name,
                "owner": user,
            },
        ).json()

    def seal_dataset(self, projectname: str, name: str, user: str):
        return requests.post(
            f"{self.base_url}/project/{quote(projectname)}/dataset/seal/",
            json={
                "name": name,
                "user": user,
            },
        ).json()
