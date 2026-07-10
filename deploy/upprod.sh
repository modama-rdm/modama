#!/bin/bash
docker compose -f docker-compose.yml -f docker-compose.prod.yml up --always-recreate-deps -d
docker compose -f docker-compose.yml -f docker-compose.prod.yml restart cloudproxy mgmtinterface-proxy
docker compose -f docker-compose.yml -f docker-compose.prod.yml logs -f --tail=100
