from flask import Flask, request
import jsonschema

from ingest_mgmt_service.share_manager import (
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
    },
    "required": ["dest_path"],
}

# yuck - in addition to being a side-effect on import, we need to make sure we
# only run this in a very specific deployment (single process - can allow for
# multiple threads though)
pm = SambaProcessManager()
cm = SambaConfigManager(
    config_path="/etc/samba/smbd-dynamic.conf",
    procman=pm,
)
sm = ShareManager(config_manager=cm)
sm.update()
pm.start()


@app.route("/share/", methods=["POST"])
def create_share():
    request_data = request.json
    jsonschema.validate(schema=CREATE_SHARE_REQUEST_SCHEMA, instance=request_data)
    share_id = sm.create_share(dest_path=request_data["dest_path"])
    # TODO: return temporary access credentials
    return {"share_id": share_id}


@app.route("/share/<share_id>/", methods=["DELETE"])
def delete_share(share_id):
    raise NotImplementedError()
