# Sourced by run.sh and cleanup.sh: defines `compose`, which runs docker compose or podman
# compose, whichever is installed (docker if both are), and `build_flags` for `compose build`.
# Set MODAMA_COMPOSE=docker or MODAMA_COMPOSE=podman to choose.

has_docker_compose() {
    command -v docker >/dev/null && docker compose version >/dev/null 2>&1
}

case "${MODAMA_COMPOSE:-}" in
    docker) runtime=docker ;;
    podman) runtime=podman ;;
    "")
        if has_docker_compose; then
            runtime=docker
        elif command -v podman >/dev/null; then
            runtime=podman
        else
            echo "The integration tests need docker compose or podman compose." >&2
            exit 1
        fi
        ;;
    *)
        echo "MODAMA_COMPOSE must be docker or podman, not ${MODAMA_COMPOSE}." >&2
        exit 1
        ;;
esac

if [ "$runtime" = docker ]; then
    compose() { docker compose "$@"; }
    build_flags=(--progress=plain)
else
    # podman-compose has no --progress
    compose() { podman compose "$@"; }
    build_flags=()
    if [ "$(podman info --format '{{.Host.Security.Rootless}}' 2>/dev/null)" = true ]; then
        echo "Warning: rootless podman can't run the tests that create shares: smbdrop marks" \
            "session folders immutable, which only root on the host may do. Run as root" \
            "or with docker for those." >&2
    fi
fi
