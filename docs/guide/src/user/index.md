# User's guide

Welcome to modama, the modular data management!

TODO: general introduction for users

## Organizing your data

### Creating a project

In modama, data is foremost organized into projects. A project is
a folder in the data management, where a list of members has access
to the contents. Its name can be associated to a project name,
project number, proposal name or number or similar outside of
this system, but it can also be descriptive, given at creation by
the project's owner.

The file permissions are set up such that all members of a project
can create new files and directories in the project, but a member cannot
change another member's files.

All members of a project can also create session folders.

### Session folders

A session folder is a destination for the data of a session at a
scientific instrument. It is good practice to scope your session
folders to a single day, such that you can easily find back
the data that matches entries in your lab notebook notes.

You can create a session folders directly using [`modama-agent`](./agent.md), or
in the web managament interface.

## The data life cycle in modama

### Getting data into modama

One option for getting data into modama is to use `modama-agent`,
which can be made available directly at the scientific instruments,
where the data is first recorded. It helps to streamline data
management tasks, so you can stay focused on running your experiment.

- [`modama-agent`](./agent.md) - uploading your data directly into the data management

### Accessing data stored in modama

There are different ways of accessing your data, and depending on what
you want to accomplish.

- Linux servers: modama mounted via NFS
- Windows/Mac OS: Samba/SMB
- Web: Nextcloud

### Sharing data with external collaborators

TODO: Nextcloud

## TODO

- [ ] general "data lifecycle in modama" documentation
    - [x] create project
    - [x] create session, upload raw data
    - [ ] preview data (libertem / image viewer / ...?)
    - [ ] process data via for example jupyterhub
    - [ ] seal raw data
    - [ ] share with others via nextcloud
- [ ] document other ingest methods
