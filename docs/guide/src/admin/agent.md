# Data ingest using `modama-agent`

## Components and concepts of the system

To understand how `modama-agent` works, let's first define some terms and components of the system:

### `modama-agent`

`modama-agent` is a small background service that provides services directly at the data generating systems. It should be run as close to the data source as possible.

`modama-agent` can run in two configurations:

- privileged: running as a background service, with its own mTLS keypair
- anonymous: no keypair, must contact privileged agent on different system to participate

There can be multiple `modama-agent` instances running per [resource](#resource), which can cooperatively provide data ingest services on all data generating systems belonging to the resource.

To support `modama-agent`, two more applications exist: `modama-agent-gui` and `modama-session-helper`. They
run in a user's session, under their uid, and ensure the user has access to the current ingest shares.

### `resource`

A `resource` is a data generating instrument, for example an electron microscope, a FIB, etc. There can be different computers involved with a single resource, for example for different detectors, sensors or cameras, which are often provided by different vendors.

### `smbdrop`

`smbdrop` is a network service that provides an ingest into modama via the smb/cifs protocol. For users, this is also known as a "network drive" or a "network share". These ingest shares can be used from all supported operating systems (Windows/Linux/Mac OS)

`smbdrop` must run on a system that has direct local access to the data, it can't provide all the needed functionality via a network file system.

### Certificate Authority (CA)

`modama-agent` in privileged configuration uses a client certificate to certify that it is part of a specific resource. It allows different computers that are part of a resource to have convenient access to the same session.

The certificate authority provides certificates for both the server (`smbdrop`) and clients (`modama-agent`). 

The CA is managed via a command line tool, that is made available via a docker container. There is a small helper script to run this container, see `deploy/ca.sh` in the repository.

- When installing `modama`, be sure to initialize the CA using `./deploy/ca.sh initialize-ca`.
- To enroll `smbdrop` or other backend services, use `./deploy/ca.sh enroll-server name1 name2 ...` - there can be multiple names in the certificate via the SubjectAltName (SAN) field. For `smbdrop`, the first name should be just `smbdrop` while the other name(s) should be the DNS names that are visible by the `modama-agent`s. Note that the `smbdrop` API to create shares is only exposed via the management API, but the token API is directly exposed via https, so it needs a certificate that matches its DNS name.
- To enroll a new agent, run `./deploy/ca.sh enroll-agent <AGENT_NAME> <RESOURCE_NAME>`. For example: `./deploy/ca.sh enroll-agent k2 holo` to enroll an agent for the K2 camera at the holo microscope.

The key material and certificates are stored under `/var/local/data-mgmt/CA/`. A typical directory tree could look like this:

```
/var/local/data-mgmt/CA/
/var/local/data-mgmt/CA/clients
/var/local/data-mgmt/CA/clients/holo
/var/local/data-mgmt/CA/clients/holo/k2.pub.pem
/var/local/data-mgmt/CA/clients/holo/k2.chain.pem
/var/local/data-mgmt/CA/clients/holo/k2.priv.pem
/var/local/data-mgmt/CA/clients/holo/k2.cert.pem
/var/local/data-mgmt/CA/servers
/var/local/data-mgmt/CA/servers/smbdrop
/var/local/data-mgmt/CA/servers/smbdrop/smbdrop.chain.pem
/var/local/data-mgmt/CA/servers/smbdrop/smbdrop.pub.pem
/var/local/data-mgmt/CA/servers/smbdrop/smbdrop.priv.pem
/var/local/data-mgmt/CA/servers/smbdrop/smbdrop.cert.pem
/var/local/data-mgmt/CA/ca
/var/local/data-mgmt/CA/ca/priv
/var/local/data-mgmt/CA/ca/priv/ca.priv.pem
/var/local/data-mgmt/CA/ca/pub
/var/local/data-mgmt/CA/ca/pub/ca.pub.pem
/var/local/data-mgmt/CA/ca/pub/ca.cert.pem
```

The individual files are used for the following:

* `CA/ca/priv/` - private part of the CA, this is _only_ used to sign other certificates, both agents and services. They are not used to provide a TLS service directly
* `CA/ca/pub/` - public part of the CA, which is used to verify both server and client certificates
* `CA/servers/*` - certificates in here are used to provide TLS-encrypted services. That's currently two services, both part of `smbdrop`: the token service (HTTPS w/ mTLS) and the websocket event stream API (WSS) that uses tokens for authentication.
* `CA/clients/*` - client certificates. `modama-agent` uses these to authenticate against the token service

Note that the directory structure allows to limit bind mounts of key material to only the required parts, i.e. it's easily possible to only allow access to the public part of the CA to any services that need it. Each server also has it's own directory.

## Security considerations

In the common case `modama-agent` should run privileged, such that users don't have access to mTLS key material. Preventing access might not always be possible, for example if the machine's user interface is running as an administrator user.

If a user obtains the key material, they might be able to "participate" in current and future session, and access ingest shares, so care should be taken to protect the files with the correct permissions.

Agents can also participate without having a private key, by talking over the local lab network to reach at least one privileged `modama-agent`. That means `modama` assumes the local lab network is a "security domain", meaning if a user can access it, they can participate in the experiment. Non-TLS encrypted HTTP is used locally, also to enable communication without having to manage DNS names of agents or fixed IP mappings.

The local lab network being the security domain also means anyone on that network can access the current shares / their credentials. This includes multiple sessions on each of the participating machines - talking to the local `modama-agent` API gives a user from a different session nothing they couldn't get by starting their own anonymous per-session `modama-agent` instance.

`modama-agent` has to put some trust into the local users of a participating system - meaning, it has to be able to assume that there is no malicious session helper impersonating another session. This might be fixed in the future, if this is possible without impacting features and complexity (and other security constraints, like background service capabilities).

The CA private key is encrypted with a user-provided password and
needs to be decrypted when enrolling a new agent or service.

### Network connections and protocols

`modama-agent` needs multiple network connections to provide its services.
Each agent listens locally:

- On port 3003 (HTTP) on the loopback interface
- (Optional) on the multicast address 239.192.0.1 port 5055 (UDP) - see service discovery below
- (Optional) on a dynamic port, providing JWTs for other participants in the local lab network

A short overview how `modama-agent` communicates:

- `modama-agent -> mgmt API`: HTTPS
- `modama-agent -> smbdrop event API`: websocket secure (wss)
- `modama-agent -> smbdrop token API`: HTTPS
- `modama-agent <-> modama-agent`: HTTP (local lab network), UDP multicast for service discovery

In addition, the operating system on which `modama-agent` is running will
communicate over CIFS with the Samba that is managed by `smbdrop` for ingest.

The backend part, `smbdrop`, provides three distinct services:

- Internal API (HTTP) - low-level API to manage shares, not concerned with auth{n,z}
- Event API (wss; websocket secure) - data management event bus; needs a token for auth
    - events inform about the state: active ingest shares, connected agents, ...
- Token API (HTTPS) - sign or verify JWTs (using the `smbdrop` keypair)

These are used by the management interface to provide an authenticated API
for managing shares:

- `mgmtinterface -> smbdrop API`: HTTP (docker-internal)
- `mgmtinterface -> smbdrop token API`: HTTPS

### Service discovery

mTLS is optional for `modama-agent`: systems without a private key should still
participate in data management. To enable this, privileged `modama-agent`s offer
a plain-HTTP service that provides JWTs in the name of the privileged `modama-agent`.
This service is advertised via UDP multicast on local interfaces.

## Administrative tasks

### Enrolling a new computer into the system 

- Create keypair and client certificate
    - `./deploy/ca.sh enroll-agent <AGENT_NAME> <RESOURCE_NAME>`
- Install the agent binary on the system
    - Linux
    - Windows

`modama-agent` is configured via a single TOML file. The minimal configuration
would look like this:

```toml
[services]

[ca]
ca_cert = """
-----BEGIN CERTIFICATE-----
MII....==
-----END CERTIFICATE-----
"""
```

A more complete configuration would look like this (URLs filled in for the deployment at ER-C,
when used via the JuNet):

```toml
listen_local_api = "localhost:3003"

[services]
event_url = "wss://er-c-smbdrop.fz-juelich.de:5052"
token_service_url = "https://er-c-smbdrop.fz-juelich.de:5053"
mgmt_api_url = "http://er-c-data-mgmt.fz-juelich.de:5001"
smb_hostname = "moellenstedt.iff.kfa-juelich.de"
smb_port = 445
smb_domain = "ER-C-INGEST"

[ca]
ca_cert = """
-----BEGIN CERTIFICATE-----
MII....==
-----END CERTIFICATE-----
"""

[client_cert]
chain = """
...
"""
private_key = """
...
"""
```

`mgmt_api_url` names the server of the management web interface, not the API itself: the
agent looks for the API below it, at `/api/` first and then at `/mgmt-api/`, and uses the
first one that supports its API version. A URL that already points at the API works as well.
The log says which one was found.

Tip: ensure newlines at the beginning and end of the multi-line certificates and keys,
otherwise you may get a `KeyMismatch` error!

The configuration is read from the following locations:

- Linux: `~/.config/modama-agent/modama-agent.toml` or `/etc/modama-agent.toml`

TODO: add system-wide locations, and other platforms

#### Linux

`modama-agent` needs some packages installed on the system to fully function:

- `gvfs-backends` and `gvfs-fuse` for mounting SMB ingest shares
- For the GUI and session helper, `glib2.0` needs to be installed (for talking to dbus)

Distributions as old as Ubuntu 18.04, Debian 10 or RHEL 8 should be supported, as should anything newer.

On Linux, the `modama-agent` service can be automatically managed by systemd. An example systemd unit follows:

```
[Unit]
Description=modama-agent - modular data management helper
Wants=network-online.target
After=network-online.target
RestartSec=10s

[Service]
User=root
Type=exec
Restart=always
ExecStart=/usr/local/bin/modama-agent --config-path=/etc/modama-agent.toml

[Install]
WantedBy=multi-user.target
```

Save this file as `/etc/systemd/system/modama-agent.service`, enable and start the service:

```bash
systemctl enable modama-agent
systemctl start modama-agent
```

If you would like to automatically start session helper services for each user,
you can also do this via systemd, using the following configuration:


```
[Unit]
Description=Modama session helper (mounts ingest shares)
# Skip display-manager greeters and other system users: they get a user
# manager too, but have no shares to mount.
ConditionUser=!@system
ConditionUser=!gdm-greeter

[Service]
ExecStart=/usr/local/bin/modama-agent-gui --agent-url http://localhost:3003/ session-helper
Restart=on-failure
RestartSec=5

[Install]
WantedBy=default.target
```

Together with `modama-agent`, this will automatically mount ingest shares for all users.
Save this file as `/etc/systemd/user/modama-session-helper.service` and run
`systemctl --global enable modama-session-helper.service` to enable it for all users.

The `ConditionUser=` lines keep the helper from starting for the display manager's login
screen, which runs its own user manager: `@system` covers greeter accounts in the system
UID range (`Debian-gdm`, `lightdm`, `sddm`), and `gdm-greeter` covers the greeter account
of newer GDM releases, whose UID lies outside that range. If another account still shows up as a session helper, add a
`ConditionUser=!<name>` line for it.

#### Windows

`modama-agent` can run as a Windows service in the background. To install it,
run:

```
modama-agent.exe install-as-service --config-path modama-agent.toml
```

The installation uses these machine-wide locations:

```text
%ProgramFiles%\modama\modama-agent\modama-agent.exe
%ProgramData%\modama\modama-agent\modama-agent.toml
%ProgramData%\modama\modama-agent\logs\modama-agent.log
```

The service is named `modama-agent`, starts automatically, and runs as the
passwordless virtual account `NT SERVICE\modama-agent`. The config including the
embedded mTLS private key are readable only by that service, SYSTEM,
and Administrators. Logs rotate at 10 MiB with five old files retained.

The service starts immediately after installation. A restrictive domain policy
can remove `Log on as a service` from virtual accounts; in that case grant the
right to `NT SERVICE\modama-agent` or `NT SERVICE\ALL SERVICES` through the
applicable policy.

You can later manage the service with the following commands:

```text
modama-agent.exe service-status
modama-agent.exe stop-service
modama-agent.exe start-service
modama-agent.exe uninstall-service
```
