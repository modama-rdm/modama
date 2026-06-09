#!/bin/bash
docker compose -f docker-compose.yml -f docker-compose.prod.yml pull
docker pull iffregistry.fz-juelich.de/weber/data-management-system/ca:latest
