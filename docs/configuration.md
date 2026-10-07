# Configuration and themes

## TUI configuration

TUI settings persist in:

```text
~/.config/mec/config.toml
```

```toml
refresh_interval_ms = 1000
theme = "msi-dark"
vim_keys = true
```

- `refresh_interval_ms` must be one of `500`, `1000`, `2000`, `5000`
- `theme` is one of `msi-dark`, `terminal`, `light`, `arctic`, `graphite` (`default` is
  accepted as an alias for `msi-dark` and normalized on save)
- `vim_keys = false` unmaps `h`/`j`/`k`/`l`; arrows, Tab, digits, `P`,
  `?`, and quit shortcuts always work
- unknown fields (including `mouse`, which is not a configuration option),
  wrong types, and malformed documents are rejected; a missing file
  loads defaults without creating anything; an invalid file falls back
  to defaults with a startup notice while monitoring still launches
- saves are atomic (temp file plus rename) with user-only permissions
- theme changes from the command palette persist across launches


Optional candidate slugs: `arctic` (Arctic Midnight), `graphite` (Graphite Violet). The default remains `msi-dark`; existing `terminal` and `light` are preserved. Palette theme choices save atomically. `mec --theme arctic` or `mec --theme graphite` overrides this session only.
