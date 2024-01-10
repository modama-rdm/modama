#!/bin/bash
curl -s -H 'Content-Type: application/json' -XPOST http://localhost:5051/share/ -d '{"uid": 1000, "gid": 1000, "dest_path": "/data/adhoc/proj1/session1", "resource": "holo"}' | python -m json.tool
