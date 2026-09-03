# Contributing to Autoruns for Linux

This project welcomes contributions and suggestions. Most contributions
require you to agree to a Contributor License Agreement (CLA) declaring that
you have the right to, and actually do, grant us the rights to use your
contribution. For details, visit <https://cla.opensource.microsoft.com>.

When you submit a pull request, a CLA bot automatically determines whether you
need to provide a CLA and decorates the pull request with the appropriate
status or instructions. You only need to complete this process once across
repositories using the Microsoft CLA.

## Prepare a change

1. Fork the repository and create a focused branch from the current default
   branch.
2. Keep each pull request limited to one coherent change and include tests for
   changed behavior.
3. Do not weaken static inspection boundaries, output escaping, alternate-root
   containment, or fail-closed handling without explaining and testing the
   security impact.
4. Update user-facing documentation when commands, output, packaging, or
   supported behavior changes.

Follow [BUILD.md](BUILD.md) to configure the Rust toolchain. Before opening a
pull request, run:

```bash
cargo test --all
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
./tests/package.sh
```

Package tests require both DEB and RPM tooling by default. Changes that affect
packaging should also be validated with native install, run, and remove tests
on the applicable minimum distribution and architecture.

## Policies and help

- Use [GitHub Issues](https://github.com/microsoft/Autoruns-for-Linux/issues)
  for bugs, feature requests, and contribution discussions.
- Follow the [Microsoft Open Source Code of Conduct](CODE_OF_CONDUCT.md).
- Report security vulnerabilities privately as described in
  [SECURITY.md](SECURITY.md), not in a public issue.
- See [SUPPORT.md](SUPPORT.md) for the project's support policy.

By contributing, you agree that your contributions will be licensed under the
repository's [MIT License](LICENSE).