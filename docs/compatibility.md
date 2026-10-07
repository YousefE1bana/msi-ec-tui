# Compatibility evidence

Format support and hardware validation are separate. A built package or a
configured CI job does not prove a physical MSI laptop works on that distro.

| Platform | Package path | Evidence / qualification |
|---|---|---|
| Ubuntu 26.04.1 x86_64 | .deb / tar | Physical GF63 Thin 11UC target in this pass; see validation record |
| Ubuntu 24.04 x86_64 / arm64 | .deb / tar / native RPM builder | Release CI is configured on native runners; aarch64 physical MSI hardware unverified |
| Debian 13+ | .deb | Expected with compatible glibc and driver; no physical Debian QA in this pass |
| Fedora with glibc >=2.39 | .rpm | Expected package/runtime path; physical support unverified |
| openSUSE with glibc >=2.39 | .rpm via zypper | Expected only; physical/package-manager installation unverified |
| Arch / compatible EndeavourOS or Manjaro | tar | Expected rolling glibc path; no physical QA; AUR metadata is generated, publication is not claimed |
| Other glibc Linux >=2.39 | tar | Depends on terminal, kernel, driver, hardware, and runtime ABI; unverified |
| Older Ubuntu/Debian/RHEL derivatives | source | Published binaries may require newer glibc; installer rejects <2.39 |
| Alpine/musl, WSL, macOS, Windows | none | Unsupported release runtime/hardware environment |

CPU assets exist for x86_64 and aarch64. No 32-bit or arbitrary architecture
mapping exists. Secure Boot must accept the correctly signed driver. KDE/GNOME
terminal launch behavior differs: the desktop entry is a standard terminal
application, not a standalone graphical window.
