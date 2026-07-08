#!/bin/sh
# Verbatim - one-click uinput permission setup (onboarding E9).
#
# Installs the udev rule that lets Verbatim's uinput text-injection backend
# open /dev/uinput as your normal user (no root at runtime, no re-login). This
# is the "Set up typing" action the Linux onboarding step and the E9 error
# offer. It is the ONLY step that needs sudo; Verbatim itself never runs as root.
#
# What it does, exactly:
#   1. copies 60-verbatim-uinput.rules to /etc/udev/rules.d/
#   2. reloads udev rules and re-triggers the uinput device
#   3. loads the uinput kernel module if it is not already loaded
#
# Idempotent: safe to re-run. Reversible: `sudo rm /etc/udev/rules.d/60-verbatim-uinput.rules`.
#
# POSIX sh (works under dash); the AppImage and Flatpak both ship this script.
set -eu

RULE_NAME="60-verbatim-uinput.rules"
DEST_DIR="/etc/udev/rules.d"
DEST="$DEST_DIR/$RULE_NAME"

# Locate the rule next to this script (AppImage/Flatpak layout) or in the repo.
SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
SRC="$SCRIPT_DIR/$RULE_NAME"
if [ ! -f "$SRC" ]; then
    echo "error: cannot find $RULE_NAME next to this script ($SCRIPT_DIR)." >&2
    echo "       run it from the packaging/linux directory or the bundled copy." >&2
    exit 1
fi

# Re-exec under sudo if we are not root, so the user is prompted once, up front.
if [ "$(id -u)" -ne 0 ]; then
    if command -v sudo >/dev/null 2>&1; then
        echo "Verbatim needs administrator rights once to install the uinput udev rule."
        exec sudo -- "$0" "$@"
    fi
    echo "error: must run as root (sudo not found). Re-run as: su -c '$0'" >&2
    exit 1
fi

echo "Installing $RULE_NAME -> $DEST"
install -d -m 0755 "$DEST_DIR"
install -m 0644 "$SRC" "$DEST"

# Reload + retrigger so the ACL applies now, without a reboot.
if command -v udevadm >/dev/null 2>&1; then
    udevadm control --reload-rules
    # Make sure the device node exists to be re-tagged (module may be unloaded).
    modprobe uinput 2>/dev/null || true
    udevadm trigger --subsystem-match=misc --sysname-match=uinput
    udevadm settle 2>/dev/null || true
else
    echo "warning: udevadm not found; rule installed but not reloaded." >&2
    echo "         reboot, or run 'udevadm control --reload-rules && udevadm trigger'." >&2
fi

# Load uinput on boot too (some distros do not autoload it).
if [ -d /etc/modules-load.d ]; then
    echo "uinput" > /etc/modules-load.d/verbatim-uinput.conf
fi

echo
echo "Done. /dev/uinput is now accessible to your desktop session."
echo "In Verbatim, use the onboarding \"test typing\" box to confirm injection works."
