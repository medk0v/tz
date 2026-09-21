#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
name="support-browser-test-$$"
image="support-openclaw-browser:test"
cleanup() {
  local status=$?
  if [[ "$status" -ne 0 ]]; then
    docker logs "$name" >&2 || true
  fi
  docker rm -f "$name" >/dev/null 2>&1 || true
  docker volume rm "$name" >/dev/null 2>&1 || true
}
trap cleanup EXIT

docker build -f "$root/Dockerfile.browser" -t "$image" "$root"
docker run -d --name "$name" --network none --read-only --cap-drop ALL \
  --security-opt no-new-privileges:true --pids-limit 256 --memory 768m --cpus 2 \
  --init --tmpfs /tmp:rw,nosuid,nodev,size=256m,mode=1777 \
  --tmpfs /dev/shm:rw,nosuid,nodev,noexec,size=128m,mode=1777 \
  -e DEBUG=pw:browser \
  -v "$name:/run/support-browser" "$image" >/dev/null
ready=false
for attempt in {1..20}; do
  if docker exec "$name" node -e 'process.exit(require("node:fs").statSync("/run/support-browser/runner.sock").isSocket()?0:1)' >/dev/null 2>&1; then
    ready=true
    break
  fi
  sleep 0.5
done
if [[ "$ready" != true ]]; then
  exit 1
fi
network=none
if [[ $# -gt 0 ]]; then network=bridge; fi
docker run --rm --init --network "$network" --cap-drop ALL \
  --security-opt no-new-privileges:true --read-only --entrypoint node \
  -v "$name:/run/support-browser:ro" -v "$root/bin:/opt/support-tools:ro" \
  "$image" /opt/support-tools/support-browser.smoke.mjs /run/support-browser/runner.sock "$@"
