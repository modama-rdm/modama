#!/bin/bash
curl -s -XDELETE http://localhost:5051/share/$*/ | python -m json.tool
