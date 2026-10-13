"""
ASGI entry point: ``uvicorn mgmtinterface_api.main:app``.

Kept out of the package's ``__init__`` on purpose. Building the app reads the
configuration and resolves the session secret, which must not happen just
because something imported the package; a test or tool importing
`mgmtinterface_api.config` would otherwise need the deployment's environment.
"""

from . import create_app

app = create_app()
