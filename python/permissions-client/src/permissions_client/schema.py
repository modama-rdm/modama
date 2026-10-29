from typing import Literal

from typing_extensions import NotRequired, TypedDict

ADHOC = "adhoc"

# A description is free text kept as `README.md` in the project or session
# folder (#112). The limit is in characters, which is what JSON schema's
# `maxLength` counts; the service reads back at most 64 KiB of the file, which
# covers 10 000 characters of any UTF-8.
DESCRIPTION_MAX_LENGTH = 10_000

_DESCRIPTION_PROPERTY = {
    "type": "string",
    "maxLength": DESCRIPTION_MAX_LENGTH,
    "description": (
        "Free text, saved as README.md in the folder. Surrounding whitespace is "
        "dropped; an empty text means no file."
    ),
}


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
        "description": _DESCRIPTION_PROPERTY,
    },
    "required": ["name", "owner", "members"],
}


class CreateProjectRequest(TypedDict):
    name: str
    owner: str
    members: list[str]
    description: NotRequired[str]


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
        "description": _DESCRIPTION_PROPERTY,
    },
    "required": ["name", "owner"],
}


class CreateDatasetRequest(TypedDict):
    name: str
    owner: str
    description: NotRequired[str]


SET_PROJECT_DESCRIPTION_REQUEST_SCHEMA = {
    "$schema": "http://json-schema.org/draft-07/schema#",
    "$id": "http://permissions.cloud-erc.fz-juelich.de/set-project-description-request.schema.json",
    "title": "SetProjectDescriptionRequest",
    "description": "The JSON payload for the PUT request to set a project's description",
    "type": "object",
    "properties": {
        "description": _DESCRIPTION_PROPERTY,
        "user": {
            "type": "string",
            "description": "Username of the requesting user",
        },
    },
    "required": ["description", "user"],
}


class SetProjectDescriptionRequest(TypedDict):
    description: str
    user: str


SET_DATASET_DESCRIPTION_REQUEST_SCHEMA = {
    "$schema": "http://json-schema.org/draft-07/schema#",
    "$id": "http://permissions.cloud-erc.fz-juelich.de/set-dataset-description-request.schema.json",
    "title": "SetDatasetDescriptionRequest",
    "description": "The JSON payload for the PUT request to set a dataset's description",
    "type": "object",
    "properties": {
        "name": {
            "type": "string",
            "description": "Name of the dataset",
        },
        "description": _DESCRIPTION_PROPERTY,
        "user": {
            "type": "string",
            "description": "Username of the requesting user",
        },
    },
    "required": ["name", "description", "user"],
}


class SetDatasetDescriptionRequest(TypedDict):
    name: str
    description: str
    user: str


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
    # The text of README.md in the project folder, or None. Absent from a
    # service older than #112.
    description: NotRequired[str | None]


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


class SetProjectDescriptionResponse(TypedDict):
    errno: Literal[0]
    project: SerializedProject


class SetDatasetDescriptionResponse(TypedDict):
    errno: Literal[0]


class SerializedDataset(TypedDict):
    name: str
    immutable: bool
    ctime: float
    mtime: float
    # The text of README.md in the dataset folder (next to `raw/`), or None.
    # Absent from a service older than #112.
    description: NotRequired[str | None]


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
