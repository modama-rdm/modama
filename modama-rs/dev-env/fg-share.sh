#!/bin/bash
curl -s http://localhost:3003/api/shares/foreground/ | python -m json.tool
