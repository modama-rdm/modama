# Ingest: ideas and requirements

* We do ingest via SMB. On the data producing systems, there needs to be as much support
  as possible/necessary to move data into the Samba share.
* Why an agent? Then our user interface can be a web interface, and the agent can perform
  actions without direct user interaction on the same system
* The agents can know which resource they belong to, so if we have multiple data producing
  systems on the local lab networks, they can work together 

    * Requirement: an ingest share should store the resource it belongs
      to, and this should be accessible via the API
    * Requirement: there should be a realtime event system where agents can subscribe
      to updates (prototype: poll the API instead)

* The agents should be deployable on most static data producing systems
    * static meaning systems that are permanently located in a lab, for example,
      excluding desktop systems as data producers for now
    * most: we can't realistically support very old Windows systems
      (with Windows 7 being a "maybe" target)
    * Requirement: easy deployment (maybe also a bit sneaky deployment)
    * Requirement: low resource usage, don't disrupt operations and don't
      trigger administrators to remove the agent from the system or create conflicts
      by using too much CPU/memory etc.
    * => I like the "single rust binary plus config file" approach for this

# Use case descriptions and user stories

## DLD (Delay-line detector) at the Holo

A user has a session at Holo and wants to use the DLD. But first, they take a
few non-time-resolved TEM images, for example using the K2, or the GIF camera.

They start the ingest client on the service PC, log in, and create a session.
After confirmation, this session is automatically mounted on the kunigunde-nuc
Linux system, on the support PC, and on the K2 PC. On the Windows machines,
the agent automatically opens the mapped network drive as a new explorer window.
On the Linux system, the agent is configured to mount the share in a way to give
access to the shared user that is used in this case.

The user takes some images using the K2, stores them directly in the session,
which appears as a network drive directly on the K2 camera PC.
Then they move in the DLD, and create a new jupyter notebook on kunigunde-nuc (from a template).
After each acquisition, the notebook stores the integrated XYT data cube into a
HDF5 file, The notebook asks the data management about the current foreground
session, and directly stores the data there (possibly in a subdirectory as named
by the user).

The user goes home and enjoys their evening.

The next morning, they come back, and find that all their DLD data acquired over
night has been safely stored in the data management. While the experiment is
torn down, the next user already comes in and starts their session in the same
fashion. The new user's session becomes the foreground session, but our DLD user
can still upload some missing data, for example from devices not integrated into
the data management (like an Oscilloscope maybe?).

Using the ingest client, the DLD user then stops the ingest share and optionally
seals the session.

# Design and Model

## Entities

* `Producer`: a computer (PC/server/...) that produces data
* `Agent`: software running (as a service, if possible) on a Producer
* `Resource`: a microscope, or in general a data-producing "device", that can
  consist of multiple `Producer`s (for example, multiple detectors per microscope)
* `Session`: a session directory, as created in the management interface
* `Share`: a temporary Samba share for a `Session`, with ephemeral credentials

## Relations

* An `Agent` belongs to a `Resource`, there can be multiple `Agent`s per resource
* There can be at most one `Share` for a `Session`
* A `Share` can optionally belong to a `Resource`, there can be multiple `Share`s per `Resource`
* There can be at most one foreground `Share` per `Resource`

## Properties

* An Agent should be resilient, and work with disruptions of the network or
  restarts of the data-producing system
    * This means, the Agent should know the state the system should be in, and
      work to reconcile reality with this state, for example mount Shares for
      active ingest sessions, or unmount those that have been removed from
      the smbdrop service

# Notes on auth{z,n}

* The `Agent` is as read-only as possible, but...
* The `Agent` needs to have a tiny bit of privileges
    * For the `Resource` for which the `Agent` is responsible, it needs to know
      the ingest share credentials
* We can try to use mTLS for this
* Keys/certs should be protected as well as possible, but we can't realistically
  prevent access to them on all `Producer`s.
    * If possible, use file permissions to protect key material