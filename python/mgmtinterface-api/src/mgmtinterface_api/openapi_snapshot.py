"""
Write the OpenAPI document to a file, for clients generated from it.

modama-agent builds its client for this API from the committed
`openapi.json` next to `setup.py`, so that a Rust build doesn't need Python.
A test fails while the committed file differs from what the app serves.

    python -m mgmtinterface_api.openapi_snapshot openapi.json
"""

import json
import sys
from pathlib import Path

from . import create_app
from .config import Settings


def render() -> str:
    # The document doesn't depend on the secret, but the app won't start
    # without one.
    settings = Settings(session_secret="openapi-snapshot")
    document = create_app(settings).openapi()
    return json.dumps(document, indent=2, ensure_ascii=False) + "\n"


def main() -> None:
    if len(sys.argv) != 2:
        sys.exit(f"usage: {sys.argv[0]} OUTPUT")
    Path(sys.argv[1]).write_text(render(), encoding="utf-8")


if __name__ == "__main__":
    main()
