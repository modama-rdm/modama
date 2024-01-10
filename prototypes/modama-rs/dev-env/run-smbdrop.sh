#!/bin/bash
docker run -eMODAMA_LOG_LEVEL=debug -eRUST_BACKTRACE=1 \
    -p5051:5051 -p5052:5052 -p5053:5053 -p445:445 \
    -it --rm --privileged \
    -v $(pwd)/certs:/certs/ \
    -v $(pwd)/etc-smbdrop/:/etc/smbdrop/ \
    -v $(pwd)/data/:/data/ \
    smbdrop-rs smbdrop \
    --ca-cert=/certs/ca/ca.cert.pem \
    --chain /certs/servers/localhost.chain.pem \
    --privkey /certs/servers/localhost.priv.pem \
    --api-listen 0.0.0.0:5051 \
    --ws-listen 0.0.0.0:5052 \
    --token-api-listen 0.0.0.0:5053
