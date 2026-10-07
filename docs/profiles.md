# Profiles

## Profile CLI

Profiles can be applied from the CLI or, after an explicit on-screen
confirmation, from the TUI Profiles screen. Both paths resolve through
the same safe transactional pipeline.

```text
mec profile list
mec profile show <PROFILE>
mec profile apply <PROFILE>
```

`mec profile list` lists the five built-ins first, then valid
custom-profile slugs in lexical order. It performs no hardware writes.

`mec profile show <PROFILE>` resolves and displays a profile in a
deterministic normalized view. It performs no hardware writes.

`mec profile apply <PROFILE>` resolves a profile and applies it only
through the safe transactional pipeline. Failures print
`MEC profile failed: ...` to stderr with a non-zero exit.

A PROFILE argument is a canonical slug: an exact built-in slug wins,
otherwise it must name a custom profile in the store.

## Built-in presets

```text
balanced          Balanced
silent            Silent
gaming            Gaming
battery-saver     Battery Saver
maximum-cooling   Maximum Cooling
```

Built-ins are capability-aware: they resolve against the currently
discovered hardware capabilities and may omit settings the hardware does
not expose. Resolution is data, not authorization — `apply_profile`
re-evaluates support, capabilities, and the hardware snapshot before any
write.

## Custom profile storage

Custom profiles live in:

```text
~/.config/mec/profiles/
```

A custom slug maps to `<slug>.toml` directly beneath that directory.
Current rules:

- lowercase ASCII letters, digits, and hyphens; 1..=64 bytes; first and
  last characters alphanumeric
- built-in slugs (`balanced`, `silent`, `gaming`, `battery-saver`,
  `maximum-cooling`) are reserved and can never shadow a built-in
- only direct-child regular `.toml` files are discovered; foreign
  extensions, directories, hidden/temp files, invalid names, and reserved
  names are ignored
- symlink profile files are ignored by listing and rejected on load
- maximum profile file size is 64 KiB
- malformed profiles fail with a typed parse error; listing does not
  parse file contents, so a malformed file is still listed and fails on
  load/show/apply
- MEC currently only reads custom profiles; there is no profile
  save/delete CLI and the store never creates the directory itself

## Profile data safety

Profiles are declarative TOML DATA ONLY. They are never sourced as shell,
executed, environment-expanded, or treated as command hooks; they contain
no raw sysfs paths and no raw EC register programs. A profile may request
only the fields implemented by the schema.

Example (every section optional, at least one setting required):

```toml
name = "Gaming"

[performance]
shift_mode = "turbo"
fan_mode = "advanced"
cooler_boost = true
super_battery = false

[battery]
charge_end_threshold = 80

[device]
keyboard_backlight = 2
```

`charge_end_threshold = 80` maps to the existing `msi-ec` typed 70/80
threshold pair. There is no independent start-threshold profile field.

## Transaction semantics

Conceptual flow:

```text
Profile
  ↓
fresh support/capability evaluation
  ↓
preview / validation
  ↓
current hardware snapshot
  ↓
transaction plan
  ↓
changed settings only
  ↓
ordered verified writes
  ↓
success
```

On failure:

```text
failure
  ↓
rollback
  ↓
reverse order
  ↓
verified rollback result
```

Notes:

- unchanged requested settings are not rewritten
- unsupported custom-profile requests fail closed
- `READ-ONLY` support state prevents profile mutation
- writes continue to use the closed typed `HardwareCommand`/write
  boundary with mandatory readback verification
- rollback failures are surfaced with typed errors; rollback is
  best-effort application-level compensation, not an atomic
  kernel-level transaction
- profile transactions for the same hardware root are serialized with a
  Linux advisory lock held for the full apply/rollback lifetime, so two
  concurrent `mec profile apply` processes cannot interleave
