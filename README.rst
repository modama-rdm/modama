Data management system
======================

System for managing microscopy data based on Nextcloud and a file server

Thoughts on management with docker
----------------------------------

Goal: Bring services up and take it down using docker-compose, manage state in
dedicated volumes / bind mounts. State is mostly contained in the database and
in the data directory. The configuration in config/config.php also contains
information that is specific to a nextcloud instance such as password salt and
secret. Furthermore it contains information about the database connection and
basic web server configuration.

Most configuration of Nextcloud is contained in the database, such as settings
for the various apps.

Using a custom image vs stock Nextcloud image
---------------------------------------------

The entry point of the stock nextcloud image is an installer/upgrader that
tries to preserve and/or upgrade an existing setup if it finds an existing
configuration.

That means installation is not performed as part of the Docker image build
process, but whenever an image is started up.

The directories with data and configuration are volumes that are declared in the
Dockerfile. The entire contents of that directory are controlled by the entry
point script. It preserves existing configuration and data, if it finds it in
this volume.

The source code of the installation is baked into the Docker image as part of
the build process. That means it will not change just by executing the
entrypoint script, but only if a new docker image is pulled or built.

If the entry point script finds a newer source code version built into the image
than the one found in the configuration and data volume, the entry point script
performs an upgrade. If it doesn't find an existing installation, it performs a
new setup.

-> No direct, reproducible control over configuration in Docker image. During
the initial run that creates a new installation, environment variables control
the configuration. After the first run, the configuration can not be changed in
the Dockerfile, but only by modifying the contents of the volume that contains
the configuration. The entrypoint script and/or running Nextcloud instance
modifies the configuration at its discretion.

The script is structured in such a way that an error introduced in a newer
version that fails to handle an existing installation properly can easily wipe
all data and configuration. That may happen at any time a new image is pulled. If
it will also wipe or mangle the database is unknown yet since that depends on
the installer script checking for existing colliding tables. The entrypoint
script will definitely try to create a new installation in the database.

That means if an update introduces an error in the entry point script that fails
to handle an existing setup correctly, it can potentially wipe the database,
wipe the data directory, and lose all configuration including password salts
etc. It contains code that is just one mistake away from deleting everything.

Furthermore, the Docker script can't recreate the state of a Nextcloud docker
image in a meaningful way. It just creates a Debian image that contains the
Nextcloud source code and all required packages, and an installation script for
Nextcloud as an entry point.

The Nextcloud web interface has write access to its source code and
configuration. That makes it VERY easy for intruders to get a permanent foothold
in a running instance once they can run code in the context of the web server!
Shutting down and restarting a container won't help since the configuration and
source code is preserved in a volume and not recreated.

A custom image would allow to build an image with a configuration that is
guaranteed to be fully determined by the Docker setup and suitable for revision
control. Furthermore, it would allow to have source and configuration read-only
for the web server and reset upon restarting. However, it would require taking
care of security updates etc manually, and upgrading to a newer version with
changed configuration would require performing the upgrade in a controlled
environment and pulling out the updated configuration in order to the add it to
the immutable Docker build. If an update is not done properly, it may leave an
inconsistent state between database (upgraded) and configuration (not upgraded)
that may be hard to recover in a worst case scenario.

Furthermore, configurations that use the "occ" command require connetions to the
running database and data volume. That means they can't be performed in the
Dockerfile.

Mitigation strategy for using the stock Docker image
====================================================

* Separate data directory and corresponding volume from source code and configuration -- this is an installation option
* Backup and snapshots of the volume that contains the configuration -- it may change or disappear at any time, and changes may have to be preserved!
* Configuration in a place that allows modification and revision control, i.e. likely a bind mount?
* Docker images with specific versions are available. --> Fallback in case a future image creates issues.

Proposed set-up
---------------

* For the beginning the stock Nextcloud image with proper volume set-up
  * NFS + bind mount for data directory
  * Bind mount (also NFS?) for configuration directory
* Later custom Debian-based Dockerfile + necessary configuration files that install and
  bring up the web server component of Nextcloud
  * Data directory on network file system through bind mount on host
* PostgreSQL image
  * Bind mount of SSD + replication on separate host(s) + frequent back-up?
  * Network file system probably not ideal for database, unless fast
    network is used?
  * Network block device?
  * The information in the DB is generally recoverable
    with some work (shares, configuration). Losing it would be a significant
    inconvenience for users, but not catastrophic.
  * The DB contains some highly
    sensitive, but easily recoverable information if "login credentials,
    save in database" is used, like planned here --> Selective backup to ignore any
    sensitive info!
* Redis image
  * Uncritical from my current understanding. Ephemeral state for locking + cache
  * Can likely be in-memory.
* File server
  * For now, build on existing infrastructure.
  * Not very sensitive, but very valuable data. Loss or irrevocable modification
    could be catastrophic.
  * Develop backup and recovery strategy ASAP
  * Protect against accidental deletion or modification by users or admins


Backup
------

Database
........

* Daily SQL dump into designated directory on storage server
* Selective dump excluding sensitive, but recoverable tables using the -T switch of pg_dump
  * exclude oc_credentials; oc_authtoken as well?
* TSM backup of the dump files (ensures rotation, older versions available)
* Later perhaps continuous archiving + warm standby using sth like DRDB etc https://www.postgresql.org/docs/9.1/different-replication-solutions.html


Nextcloud data
..............

* On storage server with TSM backup

User data (external storage)
............................

* On storage server with TSM backup

Configuration and source tree
.............................

* Should not change often
* "Master copy"
* TSM backup or rsync based differential backup against master copy?
