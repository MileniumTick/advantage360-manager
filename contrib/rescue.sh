#!/usr/bin/env bash
#
# Rescue: put a keyboard configuration file back from a dated local backup.
#
# This is the tool to reach for when a write went wrong, the keyboard was unplugged mid-write, or
# the v-Drive was closed while the computer was still writing. It verifies the backup before it
# writes anything, and it refuses to run without an explicit confirmation.
set -euo pipefail

usage() {
  cat <<'EOF'
Usage: adv360-rescue [--device PATH] [--backup-dir PATH]

Without arguments this lists the available backup sets and the profile each one covers.
To restore, run it and answer the prompt, or use the equivalent command directly:

  adv360-config --from-backup <set> restore --profile N --yes

Options:
  --device PATH       the v-Drive mount point (default: let the tool find it)
  --backup-dir PATH   where the dated backups live
  -h, --help          this text
EOF
}

device_args=()
backup_args=()
while [ $# -gt 0 ]; do
  case "$1" in
    --device) device_args=(--device "$2"); shift 2 ;;
    --backup-dir) backup_args=(--backup-dir "$2"); shift 2 ;;
    -h|--help) usage; exit 0 ;;
    *) echo "unknown argument: $1" >&2; usage; exit 2 ;;
  esac
done

if ! command -v adv360-config >/dev/null 2>&1; then
  echo "adv360-config is not on PATH; run contrib/install-omarchy.sh first, or use" >&2
  echo "target/release/adv360-config from the repository." >&2
  exit 1
fi

backup_root="${XDG_DATA_HOME:-$HOME/.local/share}/adv360-manager/backups"

echo "Backup sets:"
adv360-config "${backup_args[@]}" backups || true
echo
printf 'Backup set directory to restore from: '
read -r set_dir
if [ ! -d "$set_dir" ]; then
  echo "not a directory: $set_dir" >&2
  exit 1
fi
if [ ! -f "$set_dir/manifest.json" ]; then
  echo "no manifest.json in $set_dir; refusing to guess what to restore" >&2
  exit 1
fi

echo
echo "Files in this set:"
python3 - "$set_dir/manifest.json" <<'PY'
import json, sys
with open(sys.argv[1]) as fh:
    manifest = json.load(fh)
print(f"  created {manifest['created_at']}  scope {manifest['scope']}")
for entry in manifest["files"]:
    state = f"{entry['bytes']} bytes" if entry["existed"] else "did not exist at backup time"
    print(f"  {entry['relative_path']}  ({state})")
PY

echo
printf 'Profile to restore (0-9, or "all"): '
read -r profile
profile_args=()
if [ "$profile" != "all" ]; then
  profile_args=(--profile "$profile")
fi

echo
echo "This will write to the v-Drive. The keyboard must be mounted, and it must be ejected"
echo "before you close it again from the SmartSet shortcut."
printf 'Type "restore" to continue: '
read -r confirm
if [ "$confirm" != "restore" ]; then
  echo "Nothing was changed."
  exit 0
fi

adv360-config "${backup_args[@]}" "${device_args[@]}" \
  --from-backup "$set_dir" restore "${profile_args[@]}" --yes
echo
echo "Restore finished. Check the file with: adv360-config validate"
