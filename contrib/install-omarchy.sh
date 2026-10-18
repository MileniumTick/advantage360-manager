#!/usr/bin/env bash
#
# Install adv360-config for the current user on Omarchy (Arch Linux, Hyprland/Wayland).
#
# Design rules this script follows deliberately:
#   * never uses sudo and never writes outside $HOME;
#   * never touches the keyboard, the v-Drive, or any existing backup;
#   * is idempotent: running it twice leaves the same state;
#   * verifies every step and stops at the first failure.
set -euo pipefail

repo_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
bin_dir="${HOME}/.local/bin"
app_dir="${XDG_DATA_HOME:-$HOME/.local/share}/applications"
icon_dir="${XDG_DATA_HOME:-$HOME/.local/share}/icons/hicolor/scalable/apps"
backup_root="${XDG_DATA_HOME:-$HOME/.local/share}/adv360-manager/backups"

say() { printf '  %s\n' "$*"; }
step() { printf '\n== %s\n' "$*"; }

step "Checking the toolchain"
for tool in cargo rustc pkg-config; do
  if ! command -v "$tool" >/dev/null 2>&1; then
    echo "missing required tool: $tool" >&2
    exit 1
  fi
  say "$tool $(command -v "$tool")"
done
if ! pkg-config --exists gtk4; then
  echo "gtk4 development files not found (install gtk4 and try again)" >&2
  exit 1
fi
say "gtk4 $(pkg-config --modversion gtk4)"

step "Building the release binary"
cd "$repo_root"
cargo build --release --workspace
say "built target/release/adv360-config"

step "Installing the command"
mkdir -p "$bin_dir"
install -m 0755 target/release/adv360-config "$bin_dir/adv360-config"
say "$bin_dir/adv360-config"

step "Installing the desktop entry and icon"
mkdir -p "$app_dir" "$icon_dir"
install -m 0644 contrib/adv360-config.desktop "$app_dir/adv360-config.desktop"
install -m 0644 contrib/adv360-config.svg "$icon_dir/adv360-config.svg"
say "$app_dir/adv360-config.desktop"
say "$icon_dir/adv360-config.svg"
if command -v update-desktop-database >/dev/null 2>&1; then
  update-desktop-database "$app_dir" >/dev/null 2>&1 || true
fi
if command -v gtk-update-icon-cache >/dev/null 2>&1; then
  gtk-update-icon-cache -q -t -f "$(dirname "$(dirname "$icon_dir")")" >/dev/null 2>&1 || true
fi

step "Installing the rescue script"
install -m 0755 contrib/rescue.sh "$bin_dir/adv360-rescue"
say "$bin_dir/adv360-rescue"

step "Verifying"
"$bin_dir/adv360-config" about >/dev/null
say "adv360-config runs and reports its documented sources"
"$bin_dir/adv360-config" --help >/dev/null
say "the command line parses"

if case ":$PATH:" in *":$bin_dir:"*) true;; *) false;; esac; then
  say "$bin_dir is on PATH"
else
  echo
  echo "WARNING: $bin_dir is not on your PATH."
  echo "Add it, for example in ~/.config/fish/config.fish:"
  echo "    fish_add_path \$HOME/.local/bin"
  echo "or in ~/.bashrc / ~/.zshrc:"
  echo "    export PATH=\"\$HOME/.local/bin:\$PATH\""
fi

mkdir -p "$backup_root"

cat <<EOF

Installed.

  GUI      : adv360-config gui      (also in the application launcher)
  TUI      : adv360-config tui      (works over SSH and without a graphical session)
  Help     : adv360-config --help
  Detection: adv360-config detect
  Backups  : adv360-config backups

Backups are written to:
  $backup_root

Nothing has been written to the keyboard. Open the v-Drive from the keyboard first
(adv360-config mount-help explains how) and always eject it before closing it again.
EOF
