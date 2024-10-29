#!/bin/bash
# Token only allows pulling from the container registry
# Nothing secret there, so token is not sensitive
echo "glpat-kkyUpDWTaiCscH_NbsUT" | docker login iffregistry.fz-juelich.de --username gitlab --password-stdin

