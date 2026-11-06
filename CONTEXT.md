# Modama

Modama is a data management system for research labs whose data comes from shared
instruments: it organizes data in projects and sessions, and gets data from the instruments
into them while they are being used.

## Language

### Organizing data

**Project**:
A collection of sessions with an owner and a list of members, who all have access to its data.
_Avoid_: Group

**Owner**:
The user responsible for a project, and the only one who can change its members.

**Member**:
A user who has access to a project's data and can seal its sessions. Members can add files
and folders anywhere in the project folder except in sealed raw folders, but can only change
or remove what they added themselves.

**Session**:
One stretch of work on an instrument, usually no longer than a day, as part of a project. Its
data is kept in a session folder.
_Avoid_: Dataset

**Raw data**:
The data as an instrument recorded it, kept apart from anything derived from it.

**Description**:
The text that describes a project or a session, kept as a `README.md` in its folder. Like any
file in a project, only the user who wrote it first can change it.

**Sealing**:
Making a session's raw data immutable, so that nobody can change, rename or remove it, not
even its owner, and it can be trusted as the original record. Only administrators can unseal
it again, on request. A session whose raw data is not sealed is **open**.
_Avoid_: Freezing, locking, write-protecting, making read-only, writable (for open)

### The file system

**Project folder**:
The folder that holds a project's files. Its owner on the file system is the project's owner.

**Session folder**:
The folder that holds a session's data: its raw folder, anything derived from the raw data,
and its description. Any folder directly inside a project folder that contains a raw folder
is a session folder, however it was created.
_Avoid_: Session directory

**Raw folder**:
The `raw/` folder of a session folder, which holds its raw data and is what sealing makes
immutable.

### Instruments

**Resource**:
A data-producing instrument, such as an electron microscope, a FIB or an X-ray
diffractometer, together with all the instrument computers that belong to it.
_Avoid_: Instrument (when the whole set of computers is meant), microscope, device

**Instrument computer**:
A computer at a resource that records data, for example the one of a camera or detector;
not necessarily a desktop PC.
_Avoid_: Instrument PC, PC, producer, data generating system, client

### Getting data in

**Ingest**:
Getting data from the instrument computers into a session folder. Users upload data into an
ingest share; ingest is the whole process.
_Avoid_: Import, upload (as the name of the process)

**Ingest share**:
A temporary network share that writes into one session as the user who started it, with
credentials that are only valid until it is stopped. A user has at most one ingest share per
session; other users can have their own for the same session.
_Avoid_: Share (in user-facing text), network drive, drop, opening or closing (for starting
or stopping it)

**Ingest live**:
The status of a session that has at least one ingest share, whether or not it is sealed.
Not to be confused with sharing data with other people.

**Foreground ingest**:
The ingest share that new data at a resource should go to; at most one per resource. Starting
an ingest at a resource makes its share the foreground ingest; the user's earlier ingest
shares stay available at the resource until they are stopped.
_Avoid_: Current share, active share

**Hand-off**:
Starting an ingest at a resource whose ingest shares belong to another user: a resource has
the ingest shares of one user at a time. The other user's ingest shares are no longer
available at the resource, but are not stopped, so they keep working anywhere else.
_Avoid_: Detaching, closing (for what happens to the other user's ingest shares)

**Agent**:
The background program on an instrument computer that mounts the ingest shares of its resource
and lets users create them.
_Avoid_: Client, ingest client, daemon

**Enrolled agent**:
An agent that an administrator has enrolled for one resource. It holds a certificate naming
it and that resource, which is why it is trusted to belong to the resource.
_Avoid_: Certified agent, privileged agent

**Anonymous agent**:
An agent without a certificate, which takes part at a resource through an enrolled agent on
the same lab network. The name it reports is for display only.

**Participant**:
An agent connected to its resource and taking part in it; the same agent can briefly appear
as two participants after reconnecting.

**Session helper**:
The part of an agent that runs inside each user's login session and mounts the ingest shares
there.

**Login session**:
A user being logged in to an instrument computer, at the desktop or remotely.
_Avoid_: Session (that word means a modama session)

### Trust

**Lab network**:
The local network of a resource. Anyone who can reach it can take part in the resource's
ingest.
_Avoid_: Security domain (except when discussing trust)
