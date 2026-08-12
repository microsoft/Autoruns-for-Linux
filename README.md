# Autoruns for Linux

Autoruns for Linux lists programs, scripts, services, timers, and common persistence hooks configured to run automatically on Linux systems. It is a Rust command-line implementation inspired by Sysinternals Autoruns and Autorunsc for Windows.

This repository is in early implementation. The current scanner reports evidence from local files only; it does not execute discovered commands.

## Build

Install Rust, then build with Cargo:

```bash
cargo build --release
```

## Run

Default scan, equivalent to logon startup entries:

```bash
cargo run -- -nobanner
```

Scan all implemented Linux categories:

```bash
cargo run -- -a '*' -nobanner
```

Emit JSON:

```bash
cargo run -- -a '*' --json -nobanner
```

Scan an alternate root, useful for tests, mounted systems, containers, and offline images:

```bash
cargo run -- --root /mnt/system -a '*' -nobanner
```

## Current scanner coverage

- XDG desktop autostart entries under `/etc/xdg/autostart`, user home autostart directories, and `$HOME/.config/autostart`.
- Shell startup files such as `/etc/profile`, `/etc/profile.d/*`, and common per-user shell profiles.
- systemd services and timers under system and user unit directories.
- cron and run-parts scheduled task locations under `/etc`.
- Kernel module load configuration.
- Boot hooks such as `rc.local` and SysV init scripts.
- Dynamic loader hooks such as `/etc/ld.so.preload` and `/etc/ld.so.conf.d`.
- Network dispatcher and interface hook directories.

## Command-line options

```text
autoruns [-a <*|blnsthk>] [-c|-ct|--json|-x] [-h] [-m] [-s] [-u] [-t] [-o <output file>] [--root <path>] [-nobanner]
```

Category selectors:

- `*`: all implemented Linux categories
- `b`: boot hooks
- `h`: image hijacks and preload hooks
- `k`: dynamic loader hooks
- `l`: logon startups, the default
- `n`: network hooks
- `s`: services and module startup entries
- `t`: scheduled tasks

Some Windows-compatible Autorunsc flags are accepted but not fully implemented yet, including signature filtering, Microsoft publisher filtering, unsigned-only filtering, and VirusTotal checks. Unsupported Windows-only categories are reported explicitly rather than mapped inaccurately.

## Development plan

See [docs/implementation-plan.md](docs/implementation-plan.md) for the feature comparison, phased implementation plan, Azure Pipelines notes, and current status.

## Contributing

This project welcomes contributions and suggestions.  Most contributions require you to agree to a
Contributor License Agreement (CLA) declaring that you have the right to, and actually do, grant us
the rights to use your contribution. For details, visit https://cla.opensource.microsoft.com.

When you submit a pull request, a CLA bot will automatically determine whether you need to provide
a CLA and decorate the PR appropriately (e.g., status check, comment). Simply follow the instructions
provided by the bot. You will only need to do this once across all repos using our CLA.

This project has adopted the [Microsoft Open Source Code of Conduct](https://opensource.microsoft.com/codeofconduct/).
For more information see the [Code of Conduct FAQ](https://opensource.microsoft.com/codeofconduct/faq/) or
contact [opencode@microsoft.com](mailto:opencode@microsoft.com) with any additional questions or comments.

## Trademarks

This project may contain trademarks or logos for projects, products, or services. Authorized use of Microsoft 
trademarks or logos is subject to and must follow 
[Microsoft's Trademark & Brand Guidelines](https://www.microsoft.com/en-us/legal/intellectualproperty/trademarks/usage/general).
Use of Microsoft trademarks or logos in modified versions of this project must not cause confusion or imply Microsoft sponsorship.
Any use of third-party trademarks or logos are subject to those third-party's policies.
