#!/bin/sh
# Puts Easy Sound Cues in your apps menu, just for you (no password needed).
# Run it from the folder it came in: right-click it and choose "Run as a program", or type ./install.sh
set -e
here="$(cd "$(dirname "$0")" && pwd)"
bin="$HOME/.local/bin"
apps="$HOME/.local/share/applications"
icons="$HOME/.local/share/icons/hicolor/scalable/apps"
mkdir -p "$bin" "$apps" "$icons"
install -m 755 "$here/easy-sound-cues" "$bin/easy-sound-cues"
install -m 644 "$here/icon.svg" "$icons/easy-sound-cues.svg"
sed "s|@EXEC@|$bin/easy-sound-cues|" "$here/easy-sound-cues.desktop" > "$apps/easy-sound-cues.desktop"
gtk-update-icon-cache -f -t "$HOME/.local/share/icons/hicolor" >/dev/null 2>&1 || true
update-desktop-database "$apps" >/dev/null 2>&1 || true
echo "Done! Easy Sound Cues is now in your apps menu."
