# Building Autoruns for Linux

## Prerequisites

Install Git, a C compiler and linker, and Rust 1.97.1 with the `rustfmt` and
`clippy` components. The repository's `rust-toolchain.toml` makes rustup select
the required toolchain automatically.

On Debian or Ubuntu, the native build prerequisites can be installed with:

```bash
sudo apt-get update
sudo apt-get install -y build-essential git
```

Install Rust using the instructions at <https://rustup.rs/>.

## Build and test

From the repository root, run:

```bash
cargo build --release
cargo test --all
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
```

The release binary is written to `target/release/autoruns`.

## Build packages

Package construction requires `readelf` and the native package tooling:
`dpkg-deb` for DEB packages, and `rpmbuild` plus `rpm` for RPM packages. The
package test builds the native binary when no build directory is supplied:

```bash
./tests/package.sh
```

The test validates package metadata, architecture, payload, legal files, and
the packaged executable. Set `AUTORUNS_PACKAGE_INSTALL_TEST=1` to also install,
run, and remove the native DEB package. RPM validation is required by default.

To validate an existing build or a cross-compiled ARM64 build, pass its binary
directory and the package architecture names:

```bash
./tests/package.sh target/release amd64 x86_64
./tests/package.sh target/aarch64-unknown-linux-gnu/release arm64 aarch64
```

Cross-compilation requires the corresponding Rust target, compiler, linker,
and target system libraries. Release packages must pass validation on native
AMD64 and ARM64 build hosts; a successful cross-build alone does not establish
platform support.

See [INSTALL.md](INSTALL.md) for package installation and
[CONTRIBUTING.md](CONTRIBUTING.md) for pull request requirements.