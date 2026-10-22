# RPM spec for gander.
#
# This file is a template: `build-rpm.sh` substitutes @VERSION@ and @ARCH@ before
# rpmbuild sees it. A spec cannot take its Version from the command line, and a
# build that reads the version from a second place is a build whose package and
# binary can disagree — so the workflow passes the tag's version in explicitly.
#
# The payload is the static musl binary. It links against nothing on the host, so
# there is no runtime dependency to declare and none for rpm's dependency
# generator to find; `packaging/check-packages.sh` asserts the result is empty.

Name:           gander
Version:        @VERSION@
Release:        1
Summary:        Cross-platform interactive process viewer
License:        GPL-2.0-or-later
URL:            https://github.com/pandasir/gander
BuildArch:      @ARCH@
Source0:        gander
Source1:        LICENSE
Source2:        README.md

# No runtime dependencies at all: the binary is statically linked. The generator
# is turned off explicitly so that a future dynamically linked build cannot
# silently pick up a glibc floor and break the "works on any distribution" claim.
# (`rpmlib(...)` capabilities still appear in --requires; rpm always records the
# payload features it needs to unpack itself, and they are not package deps.)
AutoReqProv:    no

# The Rust release profile already strips the binary, so there is no debuginfo to
# split, and the distro's build-id symlinks would only add unowned files under
# /usr/lib/.build-id.
%global debug_package %{nil}
%global _build_id_links none

%description
gander is an interactive process viewer for Linux, macOS and FreeBSD in the
spirit of htop: a process table with sorting, filtering, tree view, per-process
signals, CPU affinity and I/O priority, plus header meters and a batch mode.

This package contains the statically linked binary, so it has no runtime
dependencies and does not depend on the host's glibc.

%prep
# Nothing to unpack: %{SOURCE0} is the binary itself, staged by build-rpm.sh.

%install
rm -rf %{buildroot}
install -Dm755 %{SOURCE0} %{buildroot}%{_bindir}/gander
install -Dm644 %{SOURCE1} %{buildroot}%{_datadir}/licenses/gander/LICENSE
install -Dm644 %{SOURCE2} %{buildroot}%{_docdir}/gander/README.md

%files
%{_bindir}/gander
%{_datadir}/licenses/gander/LICENSE
%{_docdir}/gander/README.md
