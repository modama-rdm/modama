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

With the default docker image, the configuration is created AFTER the image is
run for the first time, i.e. NOT as part of running the Dockerfile. That
includes creating the configuration files and initializing the database.

For a high-performance production set-up, the web server component is supposed
to be stateless so that instances can be spawned as needed to work behind a load
balancer. Redis is used for caching and locking, a network file system for the
data directory, and a database server for the database.

The default docker image contains a lot of code that may change the state. For
that reason a custom image should probably be used here.

Updating Nextcloud will likely have to be done by bringing down the web server
components, updating the docker image, updating the database with a single
instance of the new image, and then bringing up more web server components. -->
It is important that the Nextcloud version of the docker image is fixed so that
it doesn't do an uncontrolled update in case default versions change! What about
minor updates, though?

Proposed set-up
---------------

* Custom Debian-based Dockerfile + necessary configuration files that install and
  bring up the web server component of Nextcloud
  * Data directory on network file system through bind mount on host,
    or through mount within container?
  * The configuration files contain sensitive information
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

