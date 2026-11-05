# Modama - modular data management

## Cloning

This project uses git submodules. You can pull all of them automatically when cloning, like this:

```
git clone --recursive https://github.com/modama-rdm/modama.git
```

Or, after cloning, initialize and clone the submodules:

```
git submodule update --init
```

## Quickstart

To quickly get a development environment running:

```bash
cd deploy && ./uptest.sh
```

See the docs for details.

## Repository overview

- `branding/` - logo and icons
- `deploy/` - configuration for docker, docker-compose, nginx, ... for prod and dev
- `docs/` - user, administrator and developer documentation
- `mgmtinterface-new/` - the front-end part of the (new) management interface
- `modama-rs/` - components that are implemented in rust (`modama-agent{,-gui}`, `smbdrop` and supporting parts)
- `python/` - all the other services and clients, and common data structures

## Documentation

Please see [the guide](docs/guide/src/SUMMARY.md) for further details, like how to deploy
a production setup, set up a development environment, or user documentation
on various topics.

You can [build and view a copy of the documentation](docs/guide/src/development.md#documentation) by running
`mdbook serve --port 9991` or similar in the `docs/guide/` directory.
