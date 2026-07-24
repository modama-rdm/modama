from typing import Literal

from typing_extensions import TypedDict

ADHOC = "adhoc"


class InvalidAPIUsage(Exception):
    """
    Exception type for conveying the right status code to return
    """

    status_code = 400

    def __init__(self, message, status_code=None, payload=None):
        super().__init__()
        self.message = message
        if status_code is not None:
            self.status_code = status_code
        self.payload = payload

    def to_dict(self):
        rv = dict(self.payload or ())
        rv["message"] = self.message
        rv["errno"] = 1
        return rv


AUTHENTICATE_USER_REQUEST_SCHEMA = {
    "$schema": "http://json-schema.org/draft-07/schema#",
    "$id": "http://permissions.cloud-erc.fz-juelich.de/authenticate-user-request.schema.json",
    "title": "AuthenticateUserRequest",
    "description": "The JSON payload for the POST request to authenticate a user",
    "type": "object",
    "properties": {
        "username": {
            "type": "string",
            "description": "Username",
        },
        "password": {
            "type": "string",
            "description": "Password",
        },
    },
    "required": ["username", "password"],
}


class AuthenticateUserRequest(TypedDict):
    username: str
    password: str


CREATE_PROJECT_REQUEST_SCHEMA = {
    "$schema": "http://json-schema.org/draft-07/schema#",
    "$id": "http://permissions.cloud-erc.fz-juelich.de/create-project-request.schema.json",
    "title": "CreateProjectRequest",
    "description": "The JSON payload for the POST request to create a new project",
    "type": "object",
    "properties": {
        "name": {
            "type": "string",
            "description": "Short name of the project",
        },
        "owner": {
            "type": "string",
            "description": "Username of the owner of the project",
        },
        "members": {
            "type": "array",
            "items": {
                "type": "string",
            },
            "minItems": 0,
            "uniqueItems": True,
            "description": "Usernames of the members of this ad-hoc group",
        },
    },
    "required": ["name", "owner", "members"],
}


class CreateProjectRequest(TypedDict):
    name: str
    owner: str
    members: list[str]


CREATE_DATASET_REQUEST_SCHEMA = {
    "$schema": "http://json-schema.org/draft-07/schema#",
    "$id": "http://permissions.cloud-erc.fz-juelich.de/create-dataset-request.schema.json",
    "title": "CreateDatasetRequest",
    "description": "The JSON payload for the POST request to create a new dataset in a project",
    "type": "object",
    "properties": {
        "name": {
            "type": "string",
            "description": "Name of the dataset, which will be used as directory name",
        },
        "owner": {
            "type": "string",
            "description": "Username of the owner of the dataset",
        },
    },
    "required": ["name", "owner"],
}


class CreateDatasetRequest(TypedDict):
    name: str
    owner: str


SEAL_DATASET_REQUEST_SCHEMA = {
    "$schema": "http://json-schema.org/draft-07/schema#",
    "$id": "http://permissions.cloud-erc.fz-juelich.de/seal-dataset-request.schema.json",
    "title": "SealDatasetRequest",
    "description": "The JSON payload for the POST request to seal a dataset",
    "type": "object",
    "properties": {
        "name": {
            "type": "string",
            "description": "Name of the dataset",
        },
        "user": {
            "type": "string",
            "description": "Username of the requesting user",
        },
    },
    "required": ["name", "user"],
}


class SealDatasetRequest(TypedDict):
    name: str
    user: str


CHANGE_PROJECT_MEMBERS_REQUEST_SCHEMA = {
    "$schema": "http://json-schema.org/draft-07/schema#",
    "$id": "http://permissions.cloud-erc.fz-juelich.de/create-project-request.schema.json",
    "title": "CreateProjectRequest",
    "description": "The JSON payload for the POST request to change membership of a project",
    "type": "object",
    "properties": {
        "op": {
            "type": "string",
            "enum": ["add", "remove"],
            "description": "Operation: add or remove",
        },
        "members": {
            "type": "array",
            "items": {
                "type": "string",
            },
            "minItems": 1,
            "uniqueItems": True,
            "description": "Usernames of the members to add or remove from this group",
        },
        "user": {
            "type": "string",
            "description": "Username of the requesting user",
        },
    },
    "required": ["op", "members", "user"],
}


class ChangeMembersRequest(TypedDict):
    op: str
    members: list[str]
    user: str


class GetUserListResponse(TypedDict):
    errno: Literal[0]
    users: list[str]


class GenericErrorResponse(TypedDict):
    errno: int
    message: str


class SerializedProject(TypedDict):
    relpath: str
    name: str
    owner: str
    members: list[str]
    ctime: float
    mtime: float
    gid: int
    usage_bytes: int | None


class CreateProjectResponse(TypedDict):
    errno: Literal[0]
    project: SerializedProject


class IdUidInner(TypedDict):
    uid: int


class IdUserResponse(TypedDict):
    errno: Literal[0]
    id: IdUidInner


class ChangeProjectMembersResponse(TypedDict):
    errno: Literal[0]
    project: SerializedProject


class SealDatasetResponse(TypedDict):
    errno: Literal[0]


class CreateDatasetResponse(TypedDict):
    errno: Literal[0]


class SerializedDataset(TypedDict):
    name: str
    immutable: bool
    ctime: float
    mtime: float


class ProjectDetailsResponse(TypedDict):
    errno: Literal[0]
    project: SerializedProject
    datasets: list[SerializedDataset]


class ListProjectsResponse(TypedDict):
    errno: Literal[0]
    projects: list[SerializedProject]


class AuthenticateResponse(TypedDict):
    errno: Literal[0]
    id: IdUidInner
