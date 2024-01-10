#!/bin/bash
curl -s http://localhost:3003/shares/foreground/ | python -m json.tool
