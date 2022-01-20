from typing import Dict, TypedDict
from flask import Flask, request
import jsonschema

from smbdrop.share_manager import (
    SambaProcessManager, SambaConfigManager, ShareManager
)

app = Flask(__name__)


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
            "description": "The numerical Unix GID of the primary group for the user that we should impersonate",
        },
    },
    "required": ["dest_path"],
}


class CreateShareRequest(TypedDict):
    dest_path: str
    uid: int
    gid: int


# FIXME: yuck - in addition to being a side-effect on import, we need to make sure we
# only run this in a very specific deployment (single process - can allow for
# multiple threads though)
pm = SambaProcessManager()
cm = SambaConfigManager(
    config_path="/etc/samba/smb-dynamic.conf",
    procman=pm,
)
sm = ShareManager(config_manager=cm)
sm.update()
pm.start()


@app.route("/share/", methods=["POST"])
def create_share() -> Dict:
    request_data: CreateShareRequest = request.json  # type:ignore
    jsonschema.validate(schema=CREATE_SHARE_REQUEST_SCHEMA, instance=request_data)
    share = sm.create_share(
        dest_path=request_data["dest_path"],
        uid=request_data["uid"],
        gid=request_data["gid"],
    )
    return dict(share)


@app.route("/share/<share_id>/", methods=["DELETE"])
def delete_share(share_id: str) -> Dict:
    sm.delete_share(share_id)
    return {"status": "ok"}
