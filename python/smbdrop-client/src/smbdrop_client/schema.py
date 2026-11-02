from typing import Literal, NotRequired, TypedDict

CREATE_SHARE_REQUEST_SCHEMA = {
    "$schema": "http://json-schema.org/draft-07/schema#",
    "$id": "http://smbdrop.cloud-erc.fz-juelich.de/create-share-request.schema.json",
    "title": "CreateShareRequest",
    "description": "The JSON payload for the POST request to create a new smbdrop share",
    "type": "object",
    "properties": {
        "dest_path": {
            "type": "string",
            "description": "Base destination path. Should be a clean, newly created directory",
        },
        "uid": {
            "type": "number",
            "description": "The numerical Unix UID of the user that we should impersonate",
        },
        "gid": {
            "type": "number",
            "description": "The numerical Unix GID of the primary group "
            "for the user that we should impersonate",
        },
        "resource": {
            "type": "string",
            "description": "The name of the resource this share belongs to",
        },
        "created_by": {
            "type": ["string", "null"],
            "description": "Login name of the user who asked for the share",
        },
    },
    "required": ["dest_path"],
}


class CreateShareRequest(TypedDict):
    dest_path: str
    uid: int
    gid: int
    resource: str | None
    created_by: NotRequired[str | None]


class SerializedShare(TypedDict):
    resource: str | None

    share_id: str
    share_name: str
    dest_path: str
    created_timestamp: int

    # temporary credentials:
    username: str
    password: str

    # mapped to this unix user:
    uid: int
    gid: int

    # login name of the user who created the share; missing or None for shares
    # created before smbdrop recorded it:
    created_by: NotRequired[str | None]


class DeleteShareResponse(TypedDict):
    status: Literal["ok"]


ListSharesResponse = dict[str, SerializedShare]


class CreateShareResponse(TypedDict):
    share: SerializedShare
