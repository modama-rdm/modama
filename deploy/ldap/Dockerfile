# Directory server for the test deployment: the same tree as the production (FreeIPA)
# directory, with a few test users, see test-data.ldif. Not used in production.
FROM debian:13-slim

RUN --mount=type=cache,target=/var/cache/apt,sharing=locked \
    --mount=target=/var/lib/apt/lists,type=cache,sharing=locked \
    apt-get -y update && apt-get -y upgrade && \
    DEBIAN_FRONTEND=noninteractive apt-get -y install slapd ldap-utils

# The package creates an empty directory "dc=nodomain"; we bring our own
RUN rm -rf /etc/ldap/slapd.d /var/lib/ldap/*

# FreeIPA groups are groupOfNames (member, memberOf) and posixGroup (gidNumber) at the
# same time. The posixGroup of nis.schema is a structural class, which cannot be combined
# with groupOfNames, so make it auxiliary.
RUN sed '/NAME .posixGroup./,/MAY/ s/STRUCTURAL/AUXILIARY/' /etc/ldap/schema/nis.schema \
        > /etc/ldap/schema/nis-bis.schema && \
    grep -A2 "NAME .posixGroup." /etc/ldap/schema/nis-bis.schema | grep -q AUXILIARY

COPY slapd.conf /etc/ldap/slapd.conf.in
COPY test-data.ldif /test-data.ldif
COPY entrypoint.sh /entrypoint.sh

EXPOSE 389
VOLUME /var/lib/ldap

HEALTHCHECK --interval=10s --timeout=5s --start-period=10s --retries=3 \
    CMD ["ldapsearch", "-x", "-H", "ldap://127.0.0.1", "-b", "", "-s", "base", "namingContexts"]

ENTRYPOINT ["/entrypoint.sh"]
