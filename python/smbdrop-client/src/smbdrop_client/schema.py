from typing import Literal, NotRequired, TypedDict

CREATE_SHARE_REQUEST_SCHEMA = {
    "$schema": "http://json-schema.org/draft-07/schema#",
    "$id": "http://smbdrop.cloud-erc.fz-juelich.de/create-share-request.schema.json",
    "title": "CreateShareRequest",
    "description": "The JSON payload for the POST request to create a new smbdrop share",
    "type": "object",
    "properties": {
        "canonical_path": {
            "type": "string",
            "description": "The canonical path of the folder to share, like "
            "`adhoc/testproject/session2`: relative to smbdrop's data root, "
            "without `..`",
        },
        "dest_path": {
            "type": "string",
            "description": "Deprecated: the folder to share with smbdrop's data root "
            "in front. Send `canonical_path` instead; not both",
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
    "oneOf": [{"required": ["canonical_path"]}, {"required": ["dest_path"]}],
}


class CreateShareRequest(TypedDict):
    canonical_path: NotRequired[str]
    # deprecated, for callers from before `canonical_path`:
    dest_path: NotRequired[str]
    uid: int
    gid: int
    resource: str | None
    created_by: NotRequired[str | None]


class SerializedShare(TypedDict):
    resource: str | None

    share_id: str
    share_name: str
    # the shared folder as smbdrop sees it: its data root plus the canonical path
    dest_path: str
    # missing or None for shares from an smbdrop that didn't record it, or that lie
    # outside of its data root:
    canonical_path: NotRequired[str | None]
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
