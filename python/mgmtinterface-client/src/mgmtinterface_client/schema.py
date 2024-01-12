from typing_extensions import TypedDict


CREATE_SHARE_REQUEST_SCHEMA = {
    "$schema": "http://json-schema.org/draft-07/schema#",
    "$id": "http://smbdrop.cloud-erc.fz-juelich.de/create-share-request.schema.json",
    "title": "CreateShareRequest",
    "description": "The JSON payload for the POST request to create a new smbdrop share",
    "type": "object",
    "properties": {
        "project": {
            "type": "string",
            "description": "Project name",
        },
        "dataset": {
            "type": "string",
            "description": "Dataset within the project",
        },
        "resource": {
            "type": "string",
            "description": "The name of the resource the created share "
                           "should belong to",
        }
    },
    "required": ["project", "dataset"],
}


class CreateShareRequest(TypedDict):
    project: str
    dataset: str
