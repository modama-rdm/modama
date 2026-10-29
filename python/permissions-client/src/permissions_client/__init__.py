import urllib.parse
from collections.abc import Iterable
from typing import Literal

import requests
import requests.auth

from permissions_client.schema import (
    AuthenticateResponse,
    ChangeProjectMembersResponse,
    CreateDatasetResponse,
    CreateProjectResponse,
    GenericErrorResponse,
    GetUserListResponse,
    IdUidInner,
    IdUserResponse,
    ListProjectsResponse,
    ProjectDetailsResponse,
    SealDatasetResponse,
    SetDatasetDescriptionResponse,
    SetProjectDescriptionResponse,
)


def quote(s: str):
    return urllib.parse.quote(s, safe="")


class PermissionsClient:
    def __init__(
        self,
        base_url: str,
        use_auth: bool = False,
        username: str | None = None,
        password: str | None = None,
    ):
        self.base_url: str = base_url
        self.username: str | None = username
        self.password: str | None = password
        self.id: None | IdUidInner = None

    @property
    def auth(self):
        return None

    def authenticate(self, username: str, password: str) -> AuthenticateResponse:
        r = requests.post(
            f"{self.base_url}/users/auth/",
            json={
                "username": str(username),
                "password": str(password),
            },
        )
        result: AuthenticateResponse = r.json()  # pyright: ignore[reportAny]
        if result["errno"] == 0:
            self.username = username
            self.password = password
            self.id = result["id"]
        return result

    def id_user(self, name: str) -> IdUserResponse:
        return requests.get(
            f"{self.base_url}/users/id/{quote(name)}/", auth=self.auth
        ).json()

    def list_users(self) -> GetUserListResponse:
        return requests.get(f"{self.base_url}/users/list/", auth=self.auth).json()

    def list_projects(self) -> ListProjectsResponse:
        return requests.get(f"{self.base_url}/project/", auth=self.auth).json()

    def projectdetails(
        self, projectname: str, fast: bool = False
    ) -> ProjectDetailsResponse | GenericErrorResponse:
        fast_qs = "?fast=1" if fast else ""
        return requests.get(
            f"{self.base_url}/project/{quote(projectname)}/{fast_qs}", auth=self.auth
        ).json()

    def create_project(
        self,
        name: str,
        owner: str,
        members: Iterable[str],
        description: str | None = None,
    ) -> CreateProjectResponse | GenericErrorResponse:
        payload: dict[str, object] = {
            "name": name,
            "owner": owner,
            "members": list(members),
        }
        # Only when given, so that the body stays what it was for callers that
        # know nothing of descriptions.
        if description is not None:
            payload["description"] = description
        return requests.post(
            f"{self.base_url}/project/",
            json=payload,
            auth=self.auth,
        ).json()

    def change_project_members(
        self,
        projectname: str,
        op: Literal["add", "remove"],
        members: Iterable[str],
        user: str,
    ) -> ChangeProjectMembersResponse | GenericErrorResponse:
        assert op in ("add", "remove")
        return requests.patch(
            f"{self.base_url}/project/{quote(projectname)}/members/",
            json={"op": op, "members": list(members), "user": user},
            auth=self.auth,
        ).json()

    def create_dataset(
        self, projectname: str, name: str, user: str, description: str | None = None
    ) -> CreateDatasetResponse | GenericErrorResponse:
        payload: dict[str, object] = {
            "name": name,
            "owner": user,
        }
        if description is not None:
            payload["description"] = description
        result = requests.post(
            f"{self.base_url}/project/{quote(projectname)}/dataset/",
            json=payload,
            auth=self.auth,
        )
        # TODO percolate consistently through client
        result.raise_for_status()
        return result.json()

    def seal_dataset(
        self, projectname: str, name: str, user: str
    ) -> SealDatasetResponse | GenericErrorResponse:
        return requests.post(
            f"{self.base_url}/project/{quote(projectname)}/dataset/seal/",
            json={
                "name": name,
                "user": user,
            },
            auth=self.auth,
        ).json()

    def set_project_description(
        self, projectname: str, description: str, user: str
    ) -> SetProjectDescriptionResponse | GenericErrorResponse:
        """
        Write `description` to the project's README.md, as `user`. An empty
        description removes the file.
        """
        return requests.put(
            f"{self.base_url}/project/{quote(projectname)}/description/",
            json={"description": description, "user": user},
            auth=self.auth,
        ).json()

    def set_dataset_description(
        self, projectname: str, name: str, description: str, user: str
    ) -> SetDatasetDescriptionResponse | GenericErrorResponse:
        """
        Write `description` to the README.md of dataset `name`, as `user`. An
        empty description removes the file. The dataset name travels in the
        body, like for sealing.
        """
        return requests.put(
            f"{self.base_url}/project/{quote(projectname)}/dataset/description/",
            json={"name": name, "description": description, "user": user},
            auth=self.auth,
        ).json()
