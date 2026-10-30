Name:           gamemon
Version:        0.1.0
Release:        1%{?dist}
Summary:        Terminal hardware monitor for Linux
License:        LicenseRef-Proprietary
URL:            https://github.com/stoel1/GAMEMON
Source0:        %{name}-%{version}.tar.gz

BuildArch:      noarch
Requires:       bash
Requires:       coreutils
Requires:       gawk
Requires:       ncurses

%description
GAMEMON is a Bash-based terminal hardware monitor. It displays CPU,
motherboard, NVIDIA GPU, memory and fan telemetry using Linux kernel
interfaces and optional NVIDIA utilities.

%prep
%autosetup

%build
# No compilation required; GAMEMON is written in Bash.

%install
install -Dpm 0755 src/gamemon %{buildroot}%{_bindir}/gamemon
install -Dpm 0644 README.md %{buildroot}%{_docdir}/%{name}/README.md
install -Dpm 0644 CHANGELOG.md %{buildroot}%{_docdir}/%{name}/CHANGELOG.md
install -Dpm 0644 docs/architecture.md %{buildroot}%{_docdir}/%{name}/architecture.md
install -Dpm 0644 docs/rpm-packaging.md %{buildroot}%{_docdir}/%{name}/rpm-packaging.md

%files
%{_bindir}/gamemon
%{_docdir}/%{name}/README.md
%{_docdir}/%{name}/CHANGELOG.md
%{_docdir}/%{name}/architecture.md
%{_docdir}/%{name}/rpm-packaging.md

%changelog
* Fri Oct 30 2026 GAMEMON Project <noreply@github.com> - 0.1.0-1
- Initial development packaging spec.
