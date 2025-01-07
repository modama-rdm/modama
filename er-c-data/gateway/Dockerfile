FROM debian:12-slim

RUN --mount=type=cache,target=/var/cache/apt,sharing=locked \
    --mount=target=/var/lib/apt/lists,type=cache,sharing=locked \
    apt-get update && apt-get install -y nginx-full supervisor \
    python3 curl
RUN rm /etc/nginx/sites-enabled/default
RUN mkdir -p /etc/letsencrypt/live/

COPY supervisord.conf /etc/supervisor/conf.d/supervisord.conf
COPY ffdhe2048.txt /etc/nginx/dhparam
COPY default-server.conf /etc/nginx/sites-enabled/default-server.conf
COPY nginx-site.conf /etc/nginx/sites-available/site.conf
COPY nginx-start.sh /usr/bin/nginx-start.sh
COPY ./acme.sh /opt/acme.sh
COPY renew-cert.sh /usr/bin/renew-cert.sh
COPY maybe-enable-https.sh /usr/bin/maybe-enable-https.sh

# Additional config needed:
# - /etc/nginx/conf.d/servername.conf.snippet should be bind-mounted
#   into the container and contain something like `server_name foo.bar.baz;`
# - Environment variables:
#   - ACME_DOMAIN: the domain for certificate generation; should match
#     the `server_name` definition above
#   - ACME_CONTACT: the email address registered with letsencrypt
#   - ACME_SERVER: the API endpoint, letsencrypt or letsencrypt_test
#     or other values understood by acme.sh
#   - ACME_RENEW_TICK: one of 5, 60, 3600;
#     used to schedule the calls for renewal (corresponds to seconds
#     but has to be one of the choices above)

COPY exit_and_cron.py /usr/bin/exit_and_cron.py

CMD ["/usr/bin/supervisord"]
