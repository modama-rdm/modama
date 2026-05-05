# Data ingest using `modama-agent`

## Components and concepts of the system

To understand how `modama-agent` works, let's first define some terms and components of the system:

### `modama-agent`

`modama-agent` is a small background service that provides services directly at the data generating systems. It should be run as close to the data source as possible.

`modama-agent` can run in two configurations:

- privileged: running as a background service, with its own mTLS keypair
- anonymous: no keypair, must contact privileged agent on different system to participate

There can be multiple `modama-agent` instances running per [resource](#resource), which can cooperatively provide data ingest services on all data generating systems belonging to the resource.

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

Note that the directory structure allows to limit bind mounts of key material to only the required parts, i.e. it's easily possible to only allow access to the public part of the CA to any services that need it. Each server also has it's own directory.

## Security considerations

In the common case `modama-agent` should run privileged, such that users don't have access to key material. Preventing access might not always be possible, for example if the user interface is running as an administrator user.

If a user obtains the key material, they might be able to "participate" in current and future session, and access ingest shares.

Agents can also participate without having a private key, by talking over the local lab network to reach at least one privileged `modama-agent`. That means `modama` assumes the local lab network is a "security domain", meaning if a user can access it, they can participate in the experiment. Non-TLS encrypted HTTP is used locally, also to enable communication without having to manage DNS names of agents or fixed IP mappings.

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
