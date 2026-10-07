# Uninstall

How to remove MEC depends on how it was installed. Every path below
removes the program while leaving per-user data in place unless you
explicitly delete it.

Per-user data lives at:

- `~/.config/mec/config.toml`
- `~/.config/mec/profiles/`

Package removal does not automatically delete these paths. Removing MEC
also does not uninstall the upstream `msi-ec` kernel module, which is
independent of MEC.

## 1. `.deb` removal

```sh
sudo apt remove mec
```

This removes `/usr/bin/mec` and the documentation under
`/usr/share/doc/mec/`. Your `~/.config/mec/` directory stays behind.

## 2. `.rpm` removal

```sh
sudo dnf remove mec
```

Same effect: system payload removed, `~/.config/mec/` stays behind.

## 3. Portable copy removal

For the user-local installer, remove `~/.local/bin/mec`,
`~/.local/share/applications/mec.desktop`, and
`~/.local/share/icons/hicolor/scalable/apps/mec.svg` if installed.
Documentation copied by the installer is in `~/.local/share/doc/mec/`; remove it too if no longer needed.
Config and profiles are preserved. For a manually extracted copy, delete the
extracted `mec-<version>-<target>/` directory, and if you copied the
`mec` binary somewhere on your `PATH`, delete that copy too.

## 4. AUR package removal

If an AUR entry is published (publication is not claimed here), remove `mec-bin` with
your normal AUR helper like any other package. As with the `.deb` and
`.rpm` paths, `~/.config/mec/` stays behind.

## 5. Optional: remove user data

Only if you want a fully clean slate, and only after package/portable
removal, delete the per-user data explicitly:

```sh
rm -rf ~/.config/mec
```

This permanently deletes `config.toml` and everything under `profiles/`.
There is no undo, and no MEC uninstall path does this automatically.
