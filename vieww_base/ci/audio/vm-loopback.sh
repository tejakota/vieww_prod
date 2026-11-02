#!/usr/bin/env bash
# ALSA and MIDI through the kernel's own loopback devices, in a VM on a
# stock kernel (audio-backends.yml's linux-alsa-loopback leg).
#
# Why a VM: GitHub's Azure runners run a kernel built without sound
# support — there is no snd-aloop, snd-seq or snd-virtmidi to load, in
# any package (run 2026-10-23: kernel/sound absent from
# linux-modules-extra-*-azure). virtme-ng boots a stock Ubuntu kernel
# over the runner's own filesystem, so the toolchain, the checkout and
# the build cache are all already there.
#
# Usage:
#   ci/audio/vm-loopback.sh --setup   install the tools + stock kernel
#                                      (the .debs land in ~/kpkg for the
#                                      workflow's cache)
#   ci/audio/vm-loopback.sh            ALSA (snd-aloop: every sample
#                                      format, rate matching both ways),
#                                      raw MIDI (snd-virtmidi wired with
#                                      aconnect) and the ALSA sequencer's
#                                      virtual ports (AU-19)
#   ci/audio/vm-loopback.sh --seq      only the sequencer leg
#
# Verified here: bash -n, and the same modprobe/aconnect/cargo lines by
# hand on a machine with a sequencer. The virtme-ng boot itself has not
# been exercised from this sandbox (no KVM); it is the same
# virtme-ng --rw-run shape the kernel's own CI uses.
set -euo pipefail

cd "$(dirname "$0")/../.."   # vieww_base/

KPKG="$HOME/kpkg"
# The kernel the VM boots: the newest stock generic the archive offers
# (the workflow caches exactly this set, keyed by the same version).
kernel_version() {
    apt-cache search --names-only '^linux-modules-extra-[0-9.]+-[0-9]+-generic$' |
        sed -E 's/^linux-modules-extra-([0-9.]+-[0-9]+)-generic.*/\1/' |
        sort -V | tail -1
}

vng() {
    # --rw-run: the VM writes the runner's own (ephemeral) filesystem, so
    # cargo reuses the host's build cache instead of rebuilding inside
    # emulation. The in-VM script runs as root, hence HOME + PATH.
    local script="$1"
    command vng --rw-run --memory 3072 --cpus 2 \
        --kernel "/boot/vmlinuz-$KERNEL" -- \
        env HOME="$HOME" PATH="$HOME/.cargo/bin:/usr/local/bin:/usr/bin:/bin" \
            bash -euxo pipefail "$script"
}

# What runs inside the VM: modules in, loopbacks wired, tests out.
# Written to a file so quoting stays sane across the VM boundary.
vm_script() { # <which legs>
    cat > "$RUNNER_TEMP/vm-tests.sh" <<EOF
modprobe snd-aloop
modprobe snd-seq
modprobe snd-virtmidi
# The virmidi card: rawmidi devices the sequencer can wire together.
CARD="\$(grep -m1 -B1 'Virtual Raw MIDI' /proc/asound/cards | head -1 | tr -d ' :' | tail -c 3)"
echo "virmidi card \${CARD}"
# Port 0 speaks into port 1: aconnect the two sequencer ports, and the
# rawmidi devices behind them become a MIDI loopback pair.
aconnect "Virtual Raw MIDI:0" "Virtual Raw MIDI:1" ||
    aconnect "\${CARD}:0" "\${CARD}:1"
export VIEWW_REQUIRE_LOOPBACK=1
export VIEWW_MIDI_LOOP="hw:\${CARD},0>hw:\${CARD},1"
cd "$PWD"
cargo test --release -p vieww-audio --test device_loopback -- --ignored --nocapture
export VIEWW_REQUIRE_SEQ=1
cargo test --release -p vieww-audio --test live_backends alsa_seq_virtual_ports_round_trip -- --ignored --nocapture
EOF
    # The seq-only leg is the second half.
    if [ "$1" = "--seq" ]; then
        sed -i '/VIEWW_REQUIRE_LOOPBACK/,+2d; /device_loopback/d' "$RUNNER_TEMP/vm-tests.sh"
    fi
    echo "$RUNNER_TEMP/vm-tests.sh"
}

case "${1:-}" in
    --setup)
        sudo apt-get update -q
        V="$(kernel_version)"
        echo "stock kernel: $V"
        mkdir -p "$KPKG"
        if ls "$KPKG"/linux-image-"$V"-generic_*.deb >/dev/null 2>&1; then
            # Restored from the workflow's cache: install what is there.
            sudo apt-get install -y -q "$KPKG"/linux-{"image,modules,modules-extra}-"$V"-generic_*.deb
        else
            sudo apt-get install -y -q \
                linux-image-"$V"-generic linux-modules-"$V"-generic \
                linux-modules-extra-"$V"-generic
            # Keep the .debs for the cache (the archive prunes them).
            cp /var/cache/apt/archives/linux-{"image,modules,modules-extra}-"$V"-generic_*.deb \
                "$KPKG"/ 2>/dev/null || true
        fi
        # virtme-ng: packaged where the archive has it, pip where not.
        if ! sudo apt-get install -y -q virtme-ng qemu-system-x86; then
            sudo apt-get install -y -q qemu-system-x86
            python3 -m pip install --user --break-system-packages virtme-ng
        fi
        KERNEL="$V" vng --version   # the boot path is exercised below
        ;;

    ""|--seq)
        # The kernel the setup installed (or the newest present).
        KERNEL="$(ls /boot/vmlinuz-*-generic | sed -E 's|.*/vmlinuz-(.*)-generic|\1|' | sort -V | tail -1)"
        [ -n "$KERNEL" ] || { echo "::error::no stock kernel installed (run --setup)"; exit 1; }
        : "${RUNNER_TEMP:=$(mktemp -d)}"
        export RUNNER_TEMP
        vng "$(vm_script "${1:-}")"
        ;;

    *)
        echo "usage: $0 [--setup|--seq]" >&2
        exit 2
        ;;
esac
