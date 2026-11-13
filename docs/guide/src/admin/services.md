# Services of `modama`

## Core services

### permissions API

The permission API provides low-level access to filesystem operations, like reading/writing access control lists, setting sessions immutable, or handling authentication.

### `watchdog`

Most filesystem permissions are realized through ACLs, including default ACLs which get applied to newly created files and directories. For some desired ACLs this is not possible - these are applied on demand by watching for file changes via inotify, in the `watchdog` service.

### management interface (API + front-end)

The management interface provices both a human-usable GUI and an API giving access to high-level operations concerning all data management operations. The API is used both by the front-end (GUI) and by `modama-agent`.

The management interface connects to the internal API and the public token API of `smbdrop`. It also uses the permissions API for filesystem (ACL) changes.


### `smbdrop`

`smbdrop` provides ephemeral ingest samba shares for data ingest into modama.

To do its job, `smbdrop` provides three different APIs:

- an internal HTTP API used by the management interface backend. This is used both by the web frontend and by `modama-agent` to start ingest for a session. Never expose this to the outside.
- a websocket event API, which "multicasts" updates whenever an ingest share is created or removed. These updates are scoped to a specific resource. This API needs authentication via a JWT. `modama-agent`s connect to this API.
- a token service, which translates a mTLS certificate (certifying that an agent belongs to a resource) to a ephemeral JWT.

## Connected services

### nextcloud

Access to the data stored in modama via the familiar user interface of NextCloud, providing sync and share functionality.

### jupyterhub

Run data analysis with direct access to the data stored in modama.

### Network filesystem access: Linux, Windows & Mac

Access to `modama` with with a dedicated network connection for workstation / server-class hardware. For more ad-hoc access,
the samba service is made available through the intranet, for example for direct access from laptops and desktop systems.

### sftp, scp, rsync, rclone, ...

Anyone with access to the Linux servers can also use normal file synchronization tools to access their data.
