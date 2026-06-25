# syntax=docker/dockerfile:1

# --- Stage 1: build the SPA -------------------------------------------------
FROM node:22-alpine AS build
LABEL maintainer="Dieter Weber <d.weber@fz-juelich.de>"

WORKDIR /app

# Install dependencies first so this layer is cached unless the lockfile changes.
COPY package.json package-lock.json ./
RUN --mount=type=cache,target=/root/.npm npm ci

# The new UI is mounted at a sub-path on the mgmt domain (the legacy Flask UI
# keeps the domain root), so the bundle's asset URLs *and* the router basename
# must carry that prefix. VITE_BASE_PATH drives both (see vite.config.ts and
# src/router.tsx) — keep it as the single source of truth. Override the build-arg
# to mount the app elsewhere.
ARG VITE_BASE_PATH=/new/
ENV VITE_BASE_PATH=${VITE_BASE_PATH}

COPY . .
RUN npm run build

# --- Stage 2: serve the static bundle with nginx ----------------------------
FROM nginx:alpine
LABEL maintainer="Dieter Weber <d.weber@fz-juelich.de>"

# Serve the bundle from the same sub-path its asset URLs were built with, so a
# request for /new/assets/<hash>.js resolves to a real file on disk. The
# mgmt-domain proxy (mgmtinterface-proxy) forwards /new/ here; every other path
# stays with the legacy Flask app.
COPY --from=build /app/dist /usr/share/nginx/html/new
COPY nginx.conf /etc/nginx/conf.d/default.conf
