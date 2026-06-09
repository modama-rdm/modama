#!/bin/bash
# Token only allows pulling from the container registry
# Nothing secret there, so token is not sensitive
echo "glpat-e537e8m2MWsDGXczPacKSG86MQp1OjFhZwk.01.0z1rwxyvc" | docker login iffregistry.fz-juelich.de --username gitlab --password-stdin

