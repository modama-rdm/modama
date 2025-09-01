#!/bin/bash
curl -s -H 'Content-Type: application/json' -XPOST http://localhost:5051/share/ -d '{"uid": 1001, "gid": 1001, "dest_path": "/data/adhoc/proj1/sess2", "resource": "laptop"}' | python -m json.tool
