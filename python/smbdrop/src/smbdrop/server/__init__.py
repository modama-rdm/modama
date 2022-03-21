from typing import Dict
from flask import Flask, request
import jsonschema

from smbdrop_client.schema import CREATE_SHARE_REQUEST_SCHEMA, CreateShareRequest
from smbdrop.share_manager import (
    SambaProcessManager, SambaConfigManager, ShareManager
)

app = Flask(__name__)


# FIXME: yuck - in addition to being a side-effect on import, we need to make
# sure we only run this in a very specific deployment (single process - can
# allow for multiple threads though)
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


@app.route("/share/", methods=["GET"])
def list_shares() -> Dict:
    shares = sm.list()
    return shares
