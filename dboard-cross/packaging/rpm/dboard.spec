# Packages the prebuilt release binary. Built by packaging/rpm/build-rpm.sh (needs: rpmbuild).
%global debug_package %{nil}
%global __strip /bin/true

Name:           dboard
Version:        %{?pkg_version}%{!?pkg_version:0.1.0}
Release:        1%{?dist}
Summary:        Native database client for PostgreSQL, MySQL and MongoDB
License:        MIT
URL:            https://github.com/alcolopa/dboard

%description
Fast desktop client with instant cell editing and production-safety guards.

%install
install -Dm755 %{_sourcedir}/dboard %{buildroot}%{_bindir}/dboard
install -Dm644 %{_sourcedir}/dboard.desktop %{buildroot}%{_datadir}/applications/dboard.desktop
install -Dm644 %{_sourcedir}/dboard.png %{buildroot}%{_datadir}/icons/hicolor/256x256/apps/dboard.png

%files
%{_bindir}/dboard
%{_datadir}/applications/dboard.desktop
%{_datadir}/icons/hicolor/256x256/apps/dboard.png
