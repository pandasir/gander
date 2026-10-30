# RPM spec for Fedora COPR: wraps the released static musl binary instead of
# building from source. COPR's builders have no guaranteed rust module, and a
# source build there would couple every COPR rebuild to a toolchain we do not
# control; the musl archive links against nothing on the host, which is what
# lets one spec serve every chroot (x86_64 and aarch64) without conditionals.
#
# The version is pinned here on purpose: a spec that reads the version from a
# second place is a build whose package and binary can disagree (same rationale
# as packaging/rpm/gander.spec). Bump this one line after each GitHub release,
# then hit Rebuild on the package in the COPR web UI.

Name:           gander
Version:        0.6.7
Release:        1%{?dist}
Summary:        Cross-platform interactive process viewer

License:        GPL-2.0-or-later
URL:            https://github.com/pandasir/gander
Source0:        %{url}/releases/download/v%{version}/gander-%{version}-%{_arch}-unknown-linux-musl.tar.gz
Source1:        LICENSE

BuildArch:      %{_arch}
# The payload is statically linked, so the dependency generator would find
# nothing anyway — disable it explicitly so a future dynamic build cannot
# silently pick up a glibc floor (same rationale as packaging/rpm/gander.spec).
AutoReqProv:    no

%description
gander is an interactive process viewer for the terminal, like htop, for
Linux, macOS and FreeBSD. It keeps htop's keybindings and puts a live
inspector beside the process table that follows the selected process.

%prep
%autosetup -c -n gander-%{version}

%build
# Nothing to build: Source0 is the released archive with the binary inside.

%install
install -Dm755 gander %{buildroot}%{_bindir}/gander
install -Dm644 %{SOURCE1} %{buildroot}%{_licensedir}/%{name}/LICENSE

%files
%license LICENSE
%{_bindir}/gander

%changelog
%autochangelog
