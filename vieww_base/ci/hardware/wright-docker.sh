#!/usr/bin/env bash
#
# WR-15: build what `wright export --docker` makes, with this machine's own
# docker — the last mile of the container export, which no hosted CI leg in
# this repository runs. Called by ci/hardware/linux.sh as the `wright-docker`
# step (SKIPped there when no docker daemon answers); runnable by hand:
#
#   ci/hardware/wright-docker.sh [results-dir]
#
# The whole chain, end to end and offline where Wright itself is concerned:
#
#   1. build the headless wright binary
#   2. fire a scratch python project to a PASSING run — the mock provider,
#      so no network and no key (this is the proof `wright export --docker`
#      waits for)
#   3. resolve the REAL digest of python:3.12-slim with this machine's
#      docker (docker pull + inspect — the one registry round trip, made by
#      docker, not by Wright), pin it in the project's export.kdl with
#      require_digest, and export the Dockerfile
#   4. check the refusal really refuses: require_digest with no digest
#      must fail, saying how to get one
#   5. verify the signed export receipt (wright verify: signature,
#      artifact hashes, blueprint, the pinned FROM line)
#   6. docker build the image — the project's own tests run at build time —
#      and docker run it: the container's run command executes as uid 10001
#      and exits 0
#
# Needs: a docker daemon this user can reach, the pinned Rust toolchain,
# python3, and everything `wright run` itself needs on Linux (Landlock, for
# the sandboxed tools). Nothing here pushes, tags or publishes anything:
# the image is built, run, and removed.

set -uo pipefail

root="$(cd "$(dirname "$0")/../.." && pwd)"     # vieww_base
wright="$root/apps/wright"
out="${1:-$root/hardware-results/wright-docker-$(date +%Y%m%d-%H%M%S)}"
mkdir -p "$out"
summary="$out/summary.txt"
failed=0

step()  { echo; echo "==> $*"; }
pass()  { echo "PASS  $1" | tee -a "$summary"; }
fail()  { echo "FAIL  $1 (see $1.log)" | tee -a "$summary"; failed=1; }
skip()  { echo "SKIP  $1 ($2)" | tee -a "$summary"; }
note()  { echo "      $*" | tee -a "$summary"; }

image="python:3.12-slim"
tag="wright-docker-check:$(date +%s)"
port=8642
mock_pid=""

cleanup() {
    [ -n "$mock_pid" ] && kill "$mock_pid" 2>/dev/null
    docker rmi "$tag" >/dev/null 2>&1
}
trap cleanup EXIT

{
    echo "wright docker-export check $(date -u +%Y-%m-%dT%H:%M:%SZ)"
    echo "commit: $(git -C "$root" rev-parse HEAD 2>/dev/null || echo unknown)"
    echo "docker: $(docker --version 2>/dev/null || echo 'NOT FOUND')"
    echo
} > "$summary"

# ── 0. a docker daemon, or an honest skip ──────────────────────────
if ! command -v docker >/dev/null 2>&1 || ! docker info >/dev/null 2>&1; then
    skip wright-docker "no docker daemon on this machine (docker info failed)"
    cat "$summary"
    exit "$failed"
fi

# ── 1. the headless binary ─────────────────────────────────────────
step "build wright (headless)"
if (cd "$wright" && cargo build -p wright) > "$out/build.log" 2>&1; then
    pass build
else
    fail build
    cat "$summary"
    exit 1
fi
bin="$wright/target/debug/wright"

# ── 2. a scratch project, fired to a passing run by the mock ───────
export WRIGHT_HOME="$out/home"
project="$out/project"
mkdir -p "$project"
cat > "$project/blueprint.kdl" <<'KDL'
meta { name "Docker Check" }
intent { summary "A tiny project whose container export is verified by building it." confirmed "v1" }
project { language "python" }
agent Writer { goal "write the code" out "note.txt" }
models { tier cheap prefer="mock-1" }
chips { tests on }
deliverable kind="code" { check "tests pass" }
KDL
mkdir -p "$WRIGHT_HOME"
cat > "$WRIGHT_HOME/config.kdl" <<KDL
provider "mock" kind="openai" base="http://127.0.0.1:$port/v1" models="*"
KDL

step "start the mock provider (offline model) on 127.0.0.1:$port"
"$bin" mock-provider --port "$port" > "$out/mock.log" 2>&1 &
mock_pid=$!
sleep 1
if ! kill -0 "$mock_pid" 2>/dev/null; then
    fail mock-provider
    cat "$summary"
    exit 1
fi
pass mock-provider

step "fire the project (mock provider, auto-approve)"
if "$bin" run "$project" --yes > "$out/run.log" 2>&1; then
    pass run
else
    fail run
    tail -20 "$out/run.log"
    cat "$summary"
    exit 1
fi

# ── 3. the real digest, pinned in export.kdl ───────────────────────
step "resolve the real digest of $image (docker pull + inspect)"
digest=""
if docker pull -q "$image" > "$out/pull.log" 2>&1; then
    digest="$(docker inspect --format '{{json .RepoDigests}}' "$image" 2>/dev/null \
        | tr -d '[]" ' | tr ',' '\n' | head -1 | sed 's/.*@/')"
fi
if [ -n "$digest" ] && [ "${#digest}" -eq 71 ]; then
    note "digest: $digest"
    pass digest
else
    # No digest: still run the whole check unpinned (the export allows a
    # moving tag and says so); the pinning legs below are skipped.
    digest=""
    skip digest "could not resolve $image's digest (docker pull/inspect failed)"
fi

if [ -n "$digest" ]; then
    cat > "$project/export.kdl" <<KDL
export {
  docker {
    digest "$image" value="$digest"
    require_digest true
  }
}
KDL
else
    : > "$project/export.kdl"
fi

step "export the Dockerfile"
"$bin" export "$project" --docker --out "$out/export" > "$out/export.log" 2>&1
exp=$?
dockerfile="$out/export/Dockerfile"
if [ $exp -ne 0 ] || [ ! -f "$dockerfile" ]; then
    fail export
    cat "$out/export.log"
    cat "$summary"
    exit 1
fi
pass export
if [ -n "$digest" ]; then
    grep -q "FROM $image@$digest" "$dockerfile" \
        && pass from-pinned \
        || { fail from-pinned; tail -5 "$dockerfile"; }
else
    grep -q "FROM $image" "$dockerfile" \
        && pass from-tag \
        || { fail from-tag; tail -5 "$dockerfile"; }
fi

# ── 4. the refusal really refuses ──────────────────────────────────
step "require_digest with no digest must refuse"
cat > "$project/export.kdl" <<'KDL'
export {
  docker {
    require_digest true
  }
}
KDL
if "$bin" export "$project" --docker --out "$out/refused" > "$out/refuse.log" 2>&1; then
    fail refusal "the export went through without a digest"
else
    grep -q "requires a digest" "$out/refuse.log" \
        && pass refusal \
        || { fail refusal "refused, but not with the digest reason"; cat "$out/refuse.log"; }
fi
# Put the pinning config back for the receipt.
if [ -n "$digest" ]; then
    cat > "$project/export.kdl" <<KDL
export {
  docker {
    digest "$image" value="$digest"
    require_digest true
  }
}
KDL
else
    : > "$project/export.kdl"
fi

# ── 5. the signed export receipt ───────────────────────────────────
step "re-export under the pinned config and verify the receipt"
"$bin" export "$project" --docker --out "$out/export" > "$out/export2.log" 2>&1
receipt="$out/export/receipt.json"
if [ ! -f "$receipt" ]; then
    fail receipt "no receipt.json beside the export"
else
    if "$bin" verify "$receipt" "$project" > "$out/verify.log" 2>&1; then
        pass receipt
        [ -n "$digest" ] && note "receipt verifies with $image pinned by digest"
    else
        fail receipt
        cat "$out/verify.log"
    fi
fi

# ── 6. docker build (tests run at build time) and docker run ───────
step "docker build the image (the project's tests run inside the build)"
if docker build -t "$tag" -f "$dockerfile" "$project" > "$out/docker-build.log" 2>&1; then
    pass docker-build
else
    fail docker-build
    tail -30 "$out/docker-build.log"
fi

if [ "$failed" -eq 0 ]; then
    step "docker run: the container's own command, as uid 10001"
    if docker run --rm "$tag" > "$out/docker-run.log" 2>&1; then
        pass docker-run
        uid="$(docker run --rm "$tag" id -u 2>/dev/null || true)"
        note "the container ran and exited 0, as uid ${uid:-?} (the Dockerfile's USER 10001)"
    else
        fail docker-run
        tail -20 "$out/docker-run.log"
    fi
else
    skip docker-run "the image was not built"
fi

echo
echo "================ summary ================"
cat "$summary"
echo
echo "results: $out"
exit "$failed"
