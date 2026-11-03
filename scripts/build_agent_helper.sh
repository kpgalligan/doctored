#!/bin/bash
# Builds the assistant's helper, doctored-agent (agent/, a SwiftPM package on Slaude;
# designs/assistant-helper.md), then, given a destination, strips it there and signs it:
#
#   scripts/build_agent_helper.sh [DESTINATION]
#
# project.yml's "Build Agent Helper" phase passes the bundle's Contents/Helpers/doctored-agent,
# before "Write Agent Catalog" seals the bundle; `make agent` passes build/agent/doctored-agent.
# Without a destination it only builds, into build/agent-swift.
#
# DR_AGENT_BUILD_CONFIG is SwiftPM's configuration, release (the default) or debug. Every app
# configuration takes the same one, so the app builds compile Slaude once, not once per app
# configuration; `make agent-test` (`swift test`, the gate's agent-tests step) compiles it once
# more in debug on the same scratch path, and SwiftPM's lock on that path serializes the two.
# Release compiles file by file in batches (-no-whole-module-optimization -enable-batch-mode),
# not as one whole module: whole-module optimization of Slaude's ~2,400 files peaks at 10.6 GB
# in one compiler process, past the 7 GB of the publish runner, for a larger binary (38.0 MB
# stripped against 35.5 MB) and JSON only 2-21% faster. In batches the largest process peaks
# at 1.8 GB, and the whole build at 2.1 GB with three jobs. Debug is 55.9 MB and 4-17 times
# slower at parsing and writing a 2 MB history (designs/assistant-helper.md, "Build and signing").
#
# SwiftPM runs under a scrubbed environment. An Xcode script phase exports the app target's
# build settings (SDKROOT, TOOLCHAINS, SWIFT_*, MACOSX_DEPLOYMENT_TARGET, ARCHS and hundreds
# more), which would steer SwiftPM's compiler; only HOME (git and SwiftPM's caches), TMPDIR,
# DEVELOPER_DIR (the Xcode that runs the app build, when set) and a fixed PATH reach it.
#
# SwiftPM is skipped when nothing it reads changed: a stamp beside the scratch path holds the
# digest of agent/Package.swift, agent/Package.resolved, every file under agent/Sources, this
# script, the configuration and the toolchain, plus the size and time of the product it
# produced; both must match. A no-op SwiftPM build of Slaude's ~2,400 files takes 0.6-1.9 s;
# the digest takes 0.12 s. A dependency whose source the digest cannot see always runs SwiftPM:
# one being edited in place (`swift package edit`, checked out under agent/Packages) or a local
# `.package(path:)` one, which build/agent-swift/workspace-state.json records.
#
# Signing is the "Build MCP Helper" phase's: Xcode's EXPANDED_CODE_SIGN_IDENTITY, else ad hoc
# ("-"); --timestamp for a real identity; the hardened runtime; no entitlements.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SCRATCH="$ROOT/build/agent-swift"
CONFIG="${DR_AGENT_BUILD_CONFIG:-release}"
SWIFTC_FLAGS=()
case "$CONFIG" in
    release) SWIFTC_FLAGS=(-Xswiftc -no-whole-module-optimization -Xswiftc -enable-batch-mode) ;;
    debug) ;;
    *)
        echo "error: DR_AGENT_BUILD_CONFIG must be release or debug, not '$CONFIG'" >&2
        exit 1
        ;;
esac
DESTINATION="${1:-}"

SCRUBBED=(env -i "HOME=$HOME" "PATH=/usr/bin:/bin:/usr/sbin:/sbin" "TMPDIR=${TMPDIR:-/tmp}")
if [ -n "${DEVELOPER_DIR:-}" ]; then
    SCRUBBED+=("DEVELOPER_DIR=$DEVELOPER_DIR")
fi

cd "$ROOT"
PRODUCT="$SCRATCH/$CONFIG/doctored-agent"
STAMP="$SCRATCH/doctored-agent-$CONFIG.stamp"

inputs_digest() {
    {
        printf 'configuration %s\n' "$CONFIG"
        printf 'swift %s\n' "$("${SCRUBBED[@]}" xcrun --find swift)"
        "${SCRUBBED[@]}" xcrun swift --version 2>&1
        for file in agent/Package.swift agent/Package.resolved scripts/build_agent_helper.sh; do
            if [ -f "$file" ]; then shasum -a 256 "$file"; else printf 'absent %s\n' "$file"; fi
        done
        find agent/Sources -type f -print0 | LC_ALL=C sort -z | xargs -0 shasum -a 256
    } | shasum -a 256 | cut -d' ' -f1
}

product_identity() {
    stat -f '%z %m' "$PRODUCT"
}

# True when a dependency's source lies outside the digest: an edited package's state is `edited`,
# a path dependency's kind and state are `fileSystem`, a local git one's kind `localSourceControl`.
local_dependency() {
    [ -d agent/Packages ] && return 0
    [ -f "$SCRATCH/workspace-state.json" ] \
        && grep -q -E '"(name|kind)" *: *"(edited|fileSystem|localSourceControl)"' "$SCRATCH/workspace-state.json"
}

DIGEST="$(inputs_digest)"
LOCAL=0
if local_dependency; then
    # No stamp is kept for such a build either: after `swift package unedit` the digest would
    # match again and skip the build that puts the pinned revision back.
    LOCAL=1
    echo "build_agent_helper: a dependency is edited or local, so SwiftPM always runs"
    rm -f "$STAMP"
fi
if [ -f "$PRODUCT" ] && [ -f "$STAMP" ] \
    && [ "$(cat "$STAMP")" = "$DIGEST $(product_identity)" ]; then
    echo "build_agent_helper: agent/ is unchanged since the $CONFIG build of $(date -r "$STAMP" '+%F %T'); SwiftPM skipped"
else
    rm -f "$STAMP"
    # SwiftPM's release passes `-num-threads`, which batch mode cannot use; the driver ignores it
    # and says so on one `warning:` line per module, which Xcode would count as a build warning.
    # That exact line is dropped; pipefail keeps SwiftPM's exit status.
    "${SCRUBBED[@]}" xcrun swift build --package-path agent --scratch-path "$SCRATCH" \
        --configuration "$CONFIG" --product doctored-agent ${SWIFTC_FLAGS[@]+"${SWIFTC_FLAGS[@]}"} 2>&1 \
        | sed -l '/^warning: ignoring -num-threads argument; cannot multithread batch mode$/d'
    if [ ! -f "$PRODUCT" ]; then
        echo "error: SwiftPM reported success but $PRODUCT is missing" >&2
        exit 1
    fi
    if [ "$LOCAL" = 0 ]; then printf '%s %s\n' "$DIGEST" "$(product_identity)" > "$STAMP"; fi
fi

if [ -z "$DESTINATION" ]; then
    echo "build_agent_helper: built $PRODUCT"
    exit 0
fi

mkdir -p "$(dirname "$DESTINATION")"
# The linker's ad-hoc signature comes off first: strip would invalidate it and say so as a
# `warning:` line, which Xcode counts as a build warning. The copy is signed again below.
cp -f "$PRODUCT" "$DESTINATION.tmp"
codesign --remove-signature "$DESTINATION.tmp"
"${SCRUBBED[@]}" xcrun strip -x "$DESTINATION.tmp"
mv -f "$DESTINATION.tmp" "$DESTINATION"
IDENTITY="${EXPANDED_CODE_SIGN_IDENTITY:--}"
TIMESTAMP=()
if [ "$IDENTITY" != "-" ]; then TIMESTAMP=(--timestamp); fi
codesign --force --options runtime ${TIMESTAMP[@]+"${TIMESTAMP[@]}"} --sign "$IDENTITY" "$DESTINATION"
echo "build_agent_helper: $DESTINATION ($CONFIG, signed $IDENTITY)"
