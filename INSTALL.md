# Installing Autoruns for Linux

Autoruns is installed as the `autoruns` command. The `1.0.0` release package
validation matrix uses the following minimum build environments:

| Package | Distribution baseline | Architectures |
| --- | --- | --- |
| DEB | Ubuntu 20.04 | AMD64, ARM64 |
| RPM | Rocky Linux 8 | AMD64, ARM64 |
| RPM | Azure Linux 3 | AMD64, ARM64 |

Repository availability and successful package construction do not by
themselves establish support for another distribution or version.

## Install a release package

Download the package for your distribution and architecture from the project
[releases](https://github.com/microsoft/Autoruns-for-Linux/releases). Then use
the distribution's package manager so runtime dependencies are resolved.

On Debian or Ubuntu:

```bash
sudo apt install ./autoruns_1.0.0_amd64.deb
```

On RPM-based distributions:

```bash
sudo dnf install ./autoruns-1.0.0-0.x86_64.rpm
```

On Azure Linux:

```bash
sudo tdnf install ./autoruns-1.0.0-0.x86_64.rpm
```

Replace the filename with the downloaded ARM64 package when applicable. Verify
the installation with:

```bash
autoruns --help
```

## Remove Autoruns

On Debian or Ubuntu:

```bash
sudo apt remove autoruns
```

On RPM-based distributions:

```bash
sudo dnf remove autoruns
```

On Azure Linux, use `sudo tdnf remove autoruns`.

Packages distributed through `packages.microsoft.com` should be installed from
their approved distribution repository once Autoruns is published there. The
repository registration instructions are distribution-specific and will be
linked from the release notes; do not substitute a repository intended for a
different distribution or architecture.

To build from source, see [BUILD.md](BUILD.md). For help, see
[SUPPORT.md](SUPPORT.md).