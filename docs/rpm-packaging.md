# RPM packaging

The repository contains an initial RPM spec for RHEL 10. It is a starting point and has not yet been verified through a full clean build and install lifecycle.

## Build prerequisites

```bash
sudo dnf install rpm-build rpmdevtools
```

## Prepare the source tree

From the repository root:

```bash
mkdir -p ~/rpmbuild/SOURCES ~/rpmbuild/SPECS
VERSION=0.1.0
tar --transform "s,^,gamemon-${VERSION}/," \
    -czf ~/rpmbuild/SOURCES/gamemon-${VERSION}.tar.gz \
    src README.md CHANGELOG.md docs packaging
cp packaging/rpm/gamemon.spec ~/rpmbuild/SPECS/
```

## Build

```bash
rpmbuild -ba ~/rpmbuild/SPECS/gamemon.spec
```

The resulting RPMs are normally written below `~/rpmbuild/RPMS/`.

Before publishing a release, verify the spec against the actual RHEL 10 build environment, inspect the RPM payload, and test installation, execution, upgrade and removal in a disposable system or VM.
