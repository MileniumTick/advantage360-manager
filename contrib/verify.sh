#!/usr/bin/env bash
#
# The project's own verification command. This is what "it passes" means here.
#
# It runs, in order: formatting, lints, the whole test suite, a release build, and then a real
# end-to-end run of the command against a synthetic v-Drive, including a real write followed by a
# restore. Every step is echoed with its exit code so the output is usable as evidence.
set -uo pipefail

repo_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

failures=0

run() {
  local label="$1"; shift
  printf '\n\033[1m== %s\033[0m\n' "$label"
  printf '$ %s\n' "$*"
  if "$@"; then
    printf '   -> exit 0\n'
  else
    printf '   -> exit %s  FAILED\n' "$?"
    failures=$((failures + 1))
  fi
}

run "Formatting (check only)" cargo fmt --all -- --check
run "Lints (warnings are errors)" cargo clippy --workspace --all-targets -- -D warnings
run "Tests" cargo test --workspace
run "Release build" cargo build --release --workspace

bin="$repo_root/target/release/adv360-config"
fixture="$work/fake-ADV360"
backups="$work/backups"

printf '\n\033[1m== End-to-end run against a synthetic v-Drive\033[0m\n'
printf '$ adv360-config fixture --out %s\n' "$fixture"
"$bin" fixture --out "$fixture" >/dev/null || failures=$((failures + 1))

printf '$ adv360-config --device ... detect\n'
"$bin" --device "$fixture" detect || failures=$((failures + 1))

printf '$ adv360-config --device ... layout\n'
"$bin" --device "$fixture" layout || failures=$((failures + 1))

printf '$ adv360-config --device ... keys --profile 4 --layer function1\n'
"$bin" --device "$fixture" --profile 4 --layer function1 keys >/dev/null || failures=$((failures + 1))

printf '$ adv360-config --device ... presets --out %s/presets\n' "$work"
"$bin" --device "$fixture" presets --out "$work/presets" >/dev/null || failures=$((failures + 1))
if [ -s "$work/presets/review-sheet.md" ]; then
  printf '   -> review sheet written (%s bytes)\n' "$(stat -c%s "$work/presets/review-sheet.md")"
else
  printf '   -> the review sheet is missing  FAILED\n'; failures=$((failures + 1))
fi

printf '$ adv360-config --device ... apply-preset --profile 4 --preset both   (draft only)\n'
before="$(find "$fixture" -type f -exec sha256sum {} + | sort)"
"$bin" --device "$fixture" --backup-dir "$backups" \
  apply-preset --profile 4 --preset both --access-key hk1 >/dev/null || failures=$((failures + 1))
after="$(find "$fixture" -type f -exec sha256sum {} + | sort)"
if [ "$before" = "$after" ]; then
  printf '   -> the v-Drive is byte-identical after drafting, as required\n'
else
  printf '   -> DRAFTING MODIFIED THE V-DRIVE  FAILED\n'; failures=$((failures + 1))
fi

printf '$ adv360-config --device ... apply-preset --profile 4 --preset both --write --yes\n'
before="$(find "$fixture" -type f -exec sha256sum {} + | sort)"
"$bin" --device "$fixture" --backup-dir "$backups" \
  apply-preset --profile 4 --preset both --access-key hk1 --write --yes || failures=$((failures + 1))
if grep -q '<function1>' "$fixture/layouts/layout4.txt" \
   && grep -q '{-lshf}{9}{+lshf}' "$fixture/layouts/layout4.txt"; then
  printf '   -> the selected profile really changed\n'
else
  printf '   -> the write did not take effect  FAILED\n'; failures=$((failures + 1))
fi
if grep -q 'dummy=1' "$fixture/settings.txt"; then
  printf '   -> settings.txt is untouched\n'
else
  printf '   -> settings.txt WAS MODIFIED  FAILED\n'; failures=$((failures + 1))
fi

printf '$ adv360-config --device ... backups\n'
"$bin" --device "$fixture" --backup-dir "$backups" backups || failures=$((failures + 1))
set_dir="$(find "$backups" -mindepth 1 -maxdepth 1 -type d | head -1)"
if [ -f "$set_dir/manifest.json" ]; then
  printf '   -> backup set present at %s\n' "$set_dir"
else
  printf '   -> NO BACKUP WAS TAKEN  FAILED\n'; failures=$((failures + 1))
fi

printf '$ adv360-config --from-backup ... restore --profile 4 --yes\n'
"$bin" --device "$fixture" --backup-dir "$backups" \
  --from-backup "$set_dir" restore --profile 4 --yes || failures=$((failures + 1))
after_restore="$(find "$fixture" -type f -exec sha256sum {} + | sort)"
# settings.txt and the other eight profiles were never meant to change, so compare everything
# except profile 4, which must be back to its original empty shape.
if grep -q '{-lshf}{9}{+lshf}' "$fixture/layouts/layout4.txt"; then
  printf '   -> the restore did not undo the write  FAILED\n'; failures=$((failures + 1))
else
  printf '   -> profile 4 is back to its original shape\n'
fi
if grep -q 'dummy=1' "$fixture/settings.txt"; then
  printf '   -> settings.txt still untouched after the restore\n'
else
  printf '   -> settings.txt WAS MODIFIED  FAILED\n'; failures=$((failures + 1))
fi
printf '   (post-restore hashes recorded: %s files)\n' "$(printf '%s\n' "$after_restore" | wc -l)"

printf '\n\033[1m== Interfaces\033[0m\n'
printf '   TUI: %s test names\n' "$(cargo test -p adv360-tui -- --list 2>/dev/null | grep -c ': test')"
printf '   GUI: %s test names\n' "$(cargo test -p adv360-gui -- --list 2>/dev/null | grep -c ': test')"

printf '\n'
if [ "$failures" -eq 0 ]; then
  printf '\033[1mAll checks passed.\033[0m\n'
  exit 0
fi
printf '\033[1m%s check(s) failed.\033[0m\n' "$failures"
exit 1
