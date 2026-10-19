# adv360-config

Configure a **Kinesis Advantage360 SmartSet USB** keyboard from Linux, with a GTK4 desktop
interface and a terminal interface that share exactly one core.

This is for the **SmartSet** model — the one that presents a small virtual flash drive (the
"v-Drive") full of plain `.txt` files. It is **not** for the Advantage360 *Professional*, which
uses ZMK and is programmed by flashing firmware.

> **Unofficial.** This is an independent, third-party tool. It is not made, endorsed, sponsored or
> supported by Kinesis. Kinesis, Advantage360, Advantage 360 and SmartSet are trademarks of their
> respective owners, used here only to say what this tool talks to. The behaviour implemented here
> comes from the publicly published Kinesis documentation cited below; the manual PDFs are linked
> to, never redistributed. **Always back up your configuration first** (this tool does it for you)
> and use it at your own risk: it writes to real hardware via the keyboard's own configuration
> files.

## Why this exists

Kinesis ships a SmartSet App for Windows and macOS only. Their own support page says Linux users
"have the option to use onboard programming or directly edit the .txt configuration files on the
v-drive". This tool is that option, with the safety rails the hand-editing route lacks: it shows
you exactly which lines change, keeps comments and instructions it does not understand, backs up
before it writes, verifies what it wrote, and can put everything back.

Both interfaces are real applications over the same core, not two implementations:

```
                 crates/adv360-core   (model, lossless parser, validator, diff,
                        |              backup, guarded writer, presets, device discovery)
        +---------------+---------------+
        |               |               |
  adv360-tui      adv360-gui      adv360-cli
  (ratatui)        (gtk4)        (adv360-config: gui, tui, scriptable commands)
```

Neither interface parses a file, validates one, or writes to the keyboard. If they need something,
it is added to the core and both get it.

## Written against these documents

Nothing here is guessed. Every rule comes from these three official documents, each noted at the
point in the code where it is used:

| Document | Version | Used for |
|---|---|---|
| [Advantage360 SmartSet Direct Programming Guide](https://kinesis-ergo.com/wp-content/uploads/Adv360-SmartSet-Direct-Programming-Guide-Version-8-8-25.pdf) | 8-8-25 | file format, the five layer headers, remap/macro/tap-and-hold syntax, the `*` disable marker, macro limits, LED syntax, eject rules |
| [Advantage360 SmartSet Action Tokens](https://kinesis-ergo.com/wp-content/uploads/Adv360-SmartSet-Direct-Programming-Action-Tokens-v3-31-23.pdf) | 3-31-23 | every action token |
| [Advantage360 SmartSet KB360 User's Manual](https://kinesis-ergo.com/wp-content/uploads/Advantage360-SmartSet-KB360-Users-Manual-v10-12-22.pdf) | 10-12-22 | the physical layout, profile 0, the nine programmable profiles, the five layers, the hotkeys being blank by default, the factory actions per layer |

The position map this tool works from has 80 entries: the 76 keys of the two modules — 75 of them
programmable, the gear-icon SmartSet key being the documented exception — plus the four FS360
foot-pedal positions. `adv360-config about` prints the same counts, so they can be checked against
your keyboard at any time.

Where the documentation is silent or ambiguous, this tool says so instead of inventing an answer.
Three examples you will see in the interfaces:

* The factory action of the **foot pedal** positions is not published in the manual's default
  layout map, so it is reported as *uncertain* rather than assumed.
* The guide says "not all key actions support Tap-and-Hold" without listing which ones, so every
  Tap-and-Hold assignment is flagged as *unverified*, and only layer/profile actions are rejected.
* The guide §5.2 prints the Fn3 LED constant as `[lay]` while its neighbours are `[lay1]` and
  `[lay2]`. Both `lay` and `lay3` are accepted on read; the literal documented spelling is what
  this tool writes.

## What it will never do

* **Never modifies `settings.txt`.** The guide says not to. It is read, reported and preserved.
* **Never resets a profile and never touches the firmware.**
* **Profile 0 is never editable.** It is listed, clearly marked, and cannot be selected for editing.
* **Never writes without a dated, verified local backup** taken first, in the same run.
* **Never writes when validation reports an error.**
* **Never writes without an explicit confirmation** that includes how many lines will change.
* **Never deletes a line.** Removing a binding switches the line off with the documented `*`
  marker, so your original text is still there and one undo brings it back.
* **Never discards what it does not understand.** Comments and unknown instructions are preserved
  byte for byte, with their own line endings.
* **Never runs as root, and never mounts anything.** The keyboard presents its own drive.

## Install on Omarchy

```bash
git clone https://github.com/mileniumtick/advantage360-manager
cd advantage360-manager
./contrib/install-omarchy.sh
```

That builds the release binary, installs `adv360-config` into `~/.local/bin` (already on your
`PATH`), the desktop entry into `~/.local/share/applications`, the icon, and a rescue script. It
uses no `sudo` and writes nothing outside `$HOME`. Nothing is written to the keyboard.

Requirements: `rust` (1.88 or newer), `gtk4` development files, `pkg-config`. On Arch:
`sudo pacman -S rust gtk4 pkgconf`.

## Use

```bash
adv360-config gui            # GTK4 window, also in the application launcher
adv360-config tui            # terminal interface, works over SSH and with no graphical session
adv360-config detect         # what is on the v-Drive, and how it was found
adv360-config layout         # the active keyboard layout, and token -> character
adv360-config keys --profile 1 --layer function1
adv360-config validate
adv360-config --help
```

The terminal interface needs no mouse, no graphical session and no display: it reads the v-Drive
through the filesystem, so it works in a remote shell as long as the mount point is reachable.

### The workflow

1. **Open the v-Drive from the keyboard.** Hold the SmartSet key (the gear icon on the right
   module) and tap **Hotkey 3** (the key marked ③, at the left edge of the right module's QWERTY
   row). The indicator LEDs flash four times to signal that it is opening. The manual calls this
   section 6.3; `adv360-config mount-help` prints it, and both interfaces show the same
   instructions when no drive is found.
2. **Detect.** `adv360-config detect`. If the drive is somewhere unusual, pass `--device PATH`.
3. **Choose profile and layer.** Profiles 1–9 are programmable. Layer `function1` is the Fn layer,
   already reachable by holding either Fn key.
4. **Assign.** Pick a key, search the action catalogue, preview what it will type.
5. **Review.** The diff view lists each key change in words, the exact changed line numbers, and a
   unified diff. Validation groups findings into errors (which block the write) and warnings.
6. **Back up and write.** The write flow takes a dated backup, writes only the selected profile's
   layout file, verifies it by reading it back, and then tells you how to eject.
7. **Eject, then close.** Unmount the drive from Linux first, then hold SmartSet and tap Hotkey 3
   again to close the v-Drive. Doing it in the other order is what corrupts layout files; the
   Direct Programming Guide warns about exactly this.
8. **Push the changes.** This step is easy to miss and the manual is explicit about it: "Changes
   made via the SmartSet App and/or Direct Programming do not go into effect automatically."
   Closing the v-Drive pushes them — or hold SmartSet and tap **Hotkey 4** (Refresh) to push them
   while keeping the drive open for further edits. Every successful write from this tool prints
   this reminder.

## The proposed symbol and navigation layers

Your brief was specific: keep QWERTY letters, stop hunting for symbols, stop hand-crawling through
text. `adv360-config presets` generates a complete draft plus a review sheet. **Nothing is applied
automatically** — read the sheet, change what you disagree with, then apply it.

```bash
adv360-config presets                    # writes adv360-presets/ and prints the review sheet
adv360-config apply-preset --profile 4 --preset both --access-key hk1
adv360-config apply-preset --profile 4 --preset both --access-key hk1 --write --yes
```

Why these layers:

* **Symbols on `<function1>`.** Fn1 is already reachable by holding either Fn key, and its only
  factory assignments are F1–F12 on the number row. The draft leaves that entire row alone, so you
  keep every function key *and* gain the symbol set behind a chord you already have.
* **Navigation on `<function2>`.** The base layout already gives you arrows on the bottom rows and
  Home/End/Delete/Page Up/Page Down on the thumb clusters, so the draft adds what is genuinely
  missing: arrows under the home-row fingers, word movement with Ctrl, document movement with
  Ctrl+Home/End, and the matching Shift selections. Fn2 has no factory assignments, so nothing is
  lost by using it — but it has no access key either, which is why the draft also offers one.

Two syntax facts shape the whole design, both from the guide:

* Shifted characters cannot be produced by a remap. The guide requires a macro with the shift
  downstroke and upstroke around the key, so `(` is `{-lshf}{9}{+lshf}`, not `[9]`.
* "The macro trigger key is represented by the trigger action, not the physical position key." The
  drafts never give one position both a remap and a macro, so a trigger fires exactly where it
  looks like it will.

The **symbols layer for the US layout** pairs opening members on the left hand with closing members
on the right, so `[]`, `{}`, `()` and `<>` alternate hands:

| Left hand | produces | Right hand | produces |
|---|---|---|---|
| `a` | `(` | `y` | `)` |
| `s` | `[` | `u` | `]` |
| `d` | `{` | `i` | `}` |
| `f` | `<` | `o` | `>` |
| `g` | `!` | `p` | `"` |
| `q` | `=` | `h` | `,` |
| `w` | `+` | `j` | `.` |
| `e` | `-` | `k` | `/` |
| `r` | `_` | `l` | `?` |
| `t` | `*` | `;` | `\` |
| `z` | `` ` `` | `'` | `|` |
| `x` | `~` | | |
| `c` | `;` | | |
| `v` | `:` | | |
| `b` | `'` | | |
| `` ` `` | `#` | | |
| `Caps` | `@` | | |
| `HK1` | `$` | | |
| `HK2` | `%` | | |
| `HK3` | `^` | | |
| `HK4` | `&` | | |

Plus `n` → Scroll Lock, purely to keep that position out of the symbol grid.

The **navigation layer** puts arrows on `h j k l` and on `s d f g`, word movement on `a` and `t`
and on `,` and `.`, document movement on `q`/`r` and `y`/`i`, and the Shift selection variants in
the corresponding places. Page Up/Down land on `u`/`o` and `HK1`/`HK2`, Delete on `x` and `/`, and
`Caps Lock` produces nothing in this layer so you cannot hit it by accident while navigating.

**Access key options** for the navigation layer, cheapest first. `HK1`–`HK4` are blank by default
(the manual says so), which is why the first two cost nothing:

| Key | Line | Cost |
|---|---|---|
| `HK1` | `[hk1]>[fn2s]` | nothing — blank by default |
| `HK3` | `[hk3]>[fn2s]` | nothing — blank by default |
| `Caps Lock` | `[caps]>[fn2s]` | Caps Lock stops locking capitals in this profile |
| Right `Win` | `[rwin]>[fn2s]` | takes over the right Super key; check your Hyprland config |

Choose `--access-key none` to apply the navigation layer without an access key (you would then also
need to reach Fn2 some other way), or leave it out to get the recommended `HK1`.

## Keyboard layout and the token/character distinction

The guide is blunt about this in §3.3: your computer must be configured for the English (US)
layout, because other language drivers put different codes on the keys needed for `[]`, `{}` and
`>`.

So both interfaces always show two different things:

* **the token** — what the keyboard sends over USB (`obrk` is usage 0x2F, "keyboard [ and {");
* **the character** — what Linux produces from it (`[`, or `{` with Shift, on `us`).

Detection reads the running Hyprland instance with `hyprctl`, and it works even when
`HYPRLAND_INSTANCE_SIGNATURE` is not exported (a tmux pane, a systemd unit, an SSH session), by
finding the socket directory. You can always correct it by hand:

```bash
adv360-config --layout us layout
adv360-config --layout latam --variant "" layout
```

Only the `us` mapping is encoded. On any other layout the character column is shown as *uncertain*
with the reason, instead of a plausible-looking wrong symbol.

## Backups, restore and recovery

Backups go to `~/.local/share/adv360-manager/backups/<YYYY-MM-DD_HHMMSS>-<scope>/`, with a
`manifest.json` listing every file, its byte count and its SHA-256. Every copy is read back and its
hash compared before the backup is reported as usable.

```bash
adv360-config backups                    # list them
adv360-config backup --profile 4         # take one on demand
adv360-rescue                            # interactive restore
adv360-config --from-backup <set> restore --profile 4 --yes
```

**If a write fails:** the original file is still in place. The writer writes a temporary file,
flushes it, reads it back and compares, and only then renames it over the target — so a failure at
any step before the rename leaves the file untouched. The backup is unaffected either way.

**If the keyboard is unplugged mid-write:** the tool reports it as a disconnect, keeps the backup,
and prints the recovery steps. Reconnect, mount the v-Drive again, and restore.

**If a layout file is already corrupted** (usually from closing the v-Drive while the computer was
still writing): restore it from the most recent backup with `adv360-rescue`. If the file cannot even
be deleted, the Kinesis support page's advice applies — the drive may need repairing.

## What was tested, and what was not

### Against the real keyboard

An Advantage360 SmartSet was connected and the whole read/write path was exercised on it. The
results are the reason this tool is trustworthy about it, so they are stated with the raw facts.

**Reading.** Detection found the drive by structure and by its `ADV360` volume label; the firmware
is **1.0.69** with bootloader 1.3, read from `settings/settings.txt`. Nineteen text files were
found: nine layouts, nine lighting files and the settings file, plus `firmware/default.upd` and the
`System Volume Information` directory, which are reported and never touched.

**Lossless round trip.** All eighteen readable files were exported through the parser and compared
byte for byte with the drive: **18 of 18 identical**, including CRLF endings, the blank lines
between layer headers, the multi-line layer LEDs and `settings.txt`. The nineteenth is a layout file
written in ISO-8859-1; the tool refuses to decode it, leaves it untouched and says so.

**Real reversible write.** One key was changed in a spare profile: `[hk1]>[q]`, the guide's own
first example.

| Check | Result |
|---|---|
| Files changed, of nineteen | **exactly one**, verified by SHA-256 across all of them |
| Before → after | 65 bytes `d37ffff1…` → 76 bytes `c39a3d4b…` |
| Line endings | CRLF preserved |
| Read back | `hk1` → `q`, factory action `null`, line 3 |
| Backup | holds the original bytes exactly |
| **Behaviour** | **the keyboard typed `q` when that key was pressed on that profile** |
| Restore | byte for byte identical to before, all nineteen files |

That last row is the one that matters: the firmware really read a file this tool wrote.

**The documentation was wrong in four places, and the real files won.** Each is now a test in
`crates/adv360-core/tests/real_keyboard_fixtures.rs`, which holds the real content verbatim:

| The documents say | The keyboard actually does |
|---|---|
| An unused layout file holds the five layer headers | Nine lines: the headers separated by blank lines, all CRLF |
| The Fn3 layer LED token is `[lay]` (§5.2) | `[lay3]`, in every lighting file. `lay` is still accepted on read |
| — | The layer indicator carries one line per layer, which the same section asks for; it is not a duplicate |
| `settings.txt` is on the drive | It is in `settings/settings.txt` |

One documented behaviour was also confirmed rather than assumed: deleting a layout file makes the
keyboard regenerate a blank one, and the regenerated file matched the firmware's own untouched files
byte for byte.

**Not tested on hardware, and deliberately so:**

* **LED writes, Tap-and-Hold and macros** were exercised against fixtures, not against the
  keyboard. Only a plain remap was written to real hardware.
* **The symbol and navigation presets** were applied to and read back from a real layout file, but
  their keys were not typed on the keyboard.
* **Firmware is never touched.** This tool has no update path at all, by design.
* This keyboard runs **1.0.69**, older than both published releases (1.0.71 and 1.0.80). The
  guide warns that "some of the features described in this guide may require a firmware update",
  so that is worth knowing if a documented feature does not appear. Updating firmware is the
  SmartSet App's job, not this tool's.

### On the development machine

* `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, the whole
  test suite and `cargo build --release --workspace` all pass; `bash contrib/verify.sh` runs all of
  that plus a real draft, write and restore against a synthetic v-Drive.
* The core is covered for lossless round trips (CRLF, BOM, missing final newline, comments,
  unknown instructions), all nine profiles and five layers, the validation rules, ordering and
  last-one-wins precedence, Tap-and-Hold, macros with speed, multiplay, fixed and random delays,
  LED lines, the diff, backups, restore, write failures and a simulated disconnect.
* Both interfaces are driven against a synthetic v-Drive, and the GTK4 window is built and driven
  as a real widget tree under a display, not merely compiled.
* Live layout detection returns `us` from the running Hyprland instance.

### A note on files this tool cannot read

A layout file that is not valid UTF-8 is skipped and reported, never converted. That is a real case,
not a hypothetical: profile 1 of the keyboard this was tested against is ISO-8859-1 and also uses
position and action tokens that appear in no official list, so the firmware ignores every line of it
and the profile behaves as if it were empty. The tool's answer is to say exactly that and leave the
file alone, rather than to guess at a file whose syntax is not the documented one.

## Project layout

```
crates/adv360-core/   the reusable core: everything that reads, validates, backs up or writes
crates/adv360-tui/    Ratatui/Crossterm terminal interface
crates/adv360-gui/    GTK4 desktop interface
crates/adv360-cli/    the adv360-config command
contrib/              install script, rescue script, desktop entry, icon
docs/                 the interface specification both interfaces implement
odd/tasks/            the work tracking document for this feature
```

## Building from source

```bash
cargo build --release            # both interfaces
cargo build --release -p adv360-cli --no-default-features   # terminal interface only, no GTK
cargo test --workspace
```

`--no-default-features` drops the GTK4 dependency, which is what you want on a headless machine:
`adv360-config gui` then reports clearly that this build has no graphical interface, and everything
else keeps working.

## Troubleshooting

**"no Advantage360 v-Drive found"** — the keyboard presents the drive itself; nothing else can
mount it for you. `adv360-config mount-help` has the steps. You can also point the tool straight at
the folder with `--device`.

**The keyboard is in profile 0 and nothing I change takes effect** — profile 0 is non-programmable
by design, which is why this tool never lets you edit it. Switch to profile 1–9 on the keyboard
(SmartSet + a number row key) and use that profile here.

**My remap does nothing** — check the layer. A remap applies to the layer it is written under, and
`function1` only applies while an Fn key is held. The key inspector always shows which layer you
are editing.

**A macro fires from the wrong key** — the guide says a macro is triggered by the key *action*, not
its position. If you also remap some other key to the same action, both will fire it; validation
reports this as `macro-trigger-shadowed`.

**A file is reported as not valid UTF-8** — it is shown read-only rather than converted, because
converting it could destroy what the firmware wrote.

**A profile's file is unreadable or does nothing** — deleting it is a supported reset. The Direct
Programming Guide says so in §4.0: "Deleting a layout file will permanently delete its stored remaps
& macros, but the keyboard will automatically regenerate a blank layout file." That was verified on
real hardware: after deleting `layouts/layout1.txt`, the file did **not** come back while the v-Drive
stayed open, and appeared again — 65 bytes, CRLF, five layer headers separated by blank lines — after
the v-Drive was ejected, closed with SmartSet + Hotkey 3 and reopened. The regenerated file was
byte-for-byte identical to the firmware's own untouched files. Delete the file, close and reopen the
v-Drive, mount it again, and you have a clean profile you can edit normally. Keep a copy first if the
file contains anything you might want: `adv360-config backup --profile N` copies it faithfully even
when this tool cannot read it.

**The drive will not eject** — save and close every file you opened from it, then retry. The guide
lists the per-OS procedure; `adv360-config mount-help` prints the Linux command for your drive.

**I wrote a layout and nothing changed on the keyboard** — the change is on the drive but has not
been pushed. Close the v-Drive, or press SmartSet + Hotkey 4 (Refresh). See step 8 above.

**Which shortcut opens the v-Drive?** SmartSet + Hotkey 3, and the same shortcut closes it. That
is section 6.3 of the manual, which also says the drive is always named `Adv360` and that the LEDs
flash four times when it opens and twice when it closes. Nothing in this tool states a shortcut the
manual does not.

## License

MIT — see [LICENSE](LICENSE). Copyright (c) 2026 Josue Chavarria.

This project is unofficial and not affiliated with, endorsed by or supported by Kinesis. The
Kinesis manuals it was written against are linked above and are the property of their authors; this
repository does not redistribute them. "Kinesis", "Advantage360" and "SmartSet" are trademarks of
Kinesis, used here only to describe what this tool communicates with.

## Contributing

The project's own verification command is the gate:

```bash
bash contrib/verify.sh
```

It runs `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, the
whole test suite, a release build, and then a real end-to-end pass against a synthetic v-Drive:
draft without touching the drive, write with `--write --yes`, confirm `settings.txt` is untouched,
and restore. Changes that leave it red are not finished.

Every behaviour in the core carries the section of the official documentation it comes from; when
the documentation is silent the code says so (see `DefaultSource`, `TapHoldSupport`) rather than
guessing. Please keep that property.
