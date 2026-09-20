# Upgrade

How to move from one MEC release to the next depends on how MEC was
installed. All paths preserve per-user data.

Per-user data lives at:

- `~/.config/mec/config.toml`
- `~/.config/mec/profiles/`

Package upgrades never overwrite these paths. Keep your own backups
before any system change regardless.

## 1. `.deb` upgrades

Install the new `.deb` for your architecture with your normal package
tool, for example:

```sh
sudo apt install ./mec_0.9.0_amd64.deb
```

The package manager replaces `/usr/bin/mec` and the documentation
under `/usr/share/doc/mec/` and leaves `~/.config/mec/` untouched.

## 2. `.rpm` upgrades

Install the new `.rpm` for your architecture with your normal package
tool, for example:

```sh
sudo dnf install ./mec-0.9.0-1.x86_64.rpm
```

Same guarantee: system payload is replaced, `~/.config/mec/` is left
untouched.

## 3. Portable tarball upgrades

There is nothing to upgrade in place. Verify the new `SHA256SUMS` as
described in [Installation](installation.md), extract the new archive
for your architecture, and use the new `mec` binary:

```sh
sha256sum -c SHA256SUMS
tar -xzf mec-x86_64-unknown-linux-gnu.tar.gz
./mec-0.9.0-x86_64-unknown-linux-gnu/mec --version
```

Your config and profiles stay where they were; only the binary you run
changes. Remove the old extracted directory when you are done with it.

## 4. AUR package upgrades

Once an AUR entry is published (none exists yet), upgrade it with your
normal AUR helper like any other `mec-bin` update. The packaging model
is unchanged: the new GitHub release tarball for your architecture is
installed as `/usr/bin/mec` plus refreshed documentation, while
`~/.config/mec/` is preserved.

## 5. After every upgrade

Check the result the same way on every path:

```sh
mec --version
mec doctor
```

`mec --version` confirms the new version is the one running.
`mec doctor` re-evaluates the current machine read-only and reports
whether this hardware is usable or read-only under the new build. See
[Installation](installation.md) for what read-only means when the
`msi-ec` interface is missing or incompatible.
