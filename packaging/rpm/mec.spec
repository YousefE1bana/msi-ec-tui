# Binary-payload RPM spec for mec. Placeholders filled by
# scripts/build-rpm.sh: @VERSION@, @ARCH@.
#
# No %build cargo invocation: the prebuilt release binary is staged into
# SOURCES by the script. No %pre/%post/%preun/%postun lifecycle scripts:
# the package is a passive file payload (no services, udev rules, sysfs
# changes, or user/group management). Debug subpackages and stripping are
# disabled so the installed binary stays bit-identical to the tested
# release binary.
Name: mec
Version: @VERSION@
Release: 1
Summary: safe capability-aware terminal control center for MSI laptops on Linux
License: MIT
BuildArch: @ARCH@

%global debug_package %{nil}
%define __strip /bin/true
%define _build_id_links none

%description
Monitors and safely controls supported MSI laptops through the msi-ec
kernel interface. Uses current-process permissions only; unsupported
hardware stays usable in read-only mode.

%install
mkdir -p %{buildroot}/usr/bin %{buildroot}/usr/share/doc/mec
install -m755 %{_sourcedir}/mec %{buildroot}/usr/bin/mec
install -m644 %{_sourcedir}/README.md %{_sourcedir}/LICENSE %{_sourcedir}/SECURITY.md %{buildroot}/usr/share/doc/mec/

%files
/usr/bin/mec
/usr/share/doc/mec/
