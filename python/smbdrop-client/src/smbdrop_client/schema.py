from typing_extensions import TypedDict


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
    },
    "required": ["dest_path"],
}


class CreateShareRequest(TypedDict):
    dest_path: str
    uid: int
    gid: int
