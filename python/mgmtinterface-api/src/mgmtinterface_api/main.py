"""
ASGI entry point: ``uvicorn mgmtinterface_api.main:app``.

Kept out of the package's ``__init__`` deliberately. Building the app reads
configuration and resolves the session secret, and that must not happen merely
because something imported the package — a test or a tool importing
`mgmtinterface_api.config` would otherwise need the deployment's environment.
"""

from . import create_app

app = create_app()
