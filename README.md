# Autoruns for Linux

Autoruns for Linux inventories programs, modules, scripts, services, extensions,
and other integrations configured to activate at Linux system, user, browser,
device, mount, network, and application events. It is a Rust command-line
implementation inspired by Sysinternals Autoruns and Autorunsc for Windows.

The scanner performs static file inspection. It never executes a discovered
command, starts a unit, synthesizes a device event, mounts media, or loads an
extension. Results preserve the relationship between an event, activation
mechanism, target, principal/profile, effective status, source evidence, and
known completeness boundary.

## Build

Install Rust, then build with Cargo:

```bash
cargo build --release
```

Pull requests targeting `main` run formatting, Clippy, tests, native DEB/RPM
package validation, and ARM64 build and package validation in CI.

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

CSV (`-c`), TSV (`-ct`), JSON (`--json`), XML (`-x`), and aligned table output
share the same fields. The original 12 machine-readable columns remain first;
event, mechanism, principal, profile, activator, target, and completeness are
followed by `TargetState`, `TargetExists`, and `TargetExecutable`. Concrete
absolute image paths are checked inside the selected root and reported as
`present`, `missing`, or `inaccessible`; relative commands are `unresolved`.
Registration `Status` remains independent, so an enabled stale entry is still
shown as enabled with `TargetState=missing`.

Scan an alternate root, useful for tests, mounted systems, containers, and offline images:

```bash
cargo run -- --root /mnt/system -a '*' -nobanner
```

## Current scanner coverage

- Effective XDG desktop autostart and system/per-user shell startup files.
- Effective systemd system and user services, timers, sockets, paths, devices,
	mounts, and automounts, including precedence, masks, drop-ins, dependencies,
	template instances, `ExecCondition`, and every condition/pre/start/post phase
	for direct and indirectly activated services.
- System and per-user cron, anacron, and eligible run-parts jobs.
- Effective modules-load configuration, boot/SysV hooks, dynamic-loader
	preload/search configuration (including in-root includes), alternatives, and
	supported network hook directories.
- Chromium-family and Firefox extensions across supported installations,
	users, and profiles; enterprise policies, external registrations, persistent
	`--load-extension` launchers, native-messaging hosts, and Firefox PKCS #11
	registrations. Native hosts are callable integrations, not browser-startup
	execution.
- Effective udev executable/unit actions, fstab `x-systemd.*` relationships,
	autofs program maps, effective systemd device/mount/path activation, and
	freedesktop autorun/autoopen precedence and contained target evidence on
	already mounted media.
- Shared and per-user LibreOffice/OpenOffice OXT packages, unpacked extensions,
	UNO components, event/job configuration, macro libraries, and native helpers.

Each selected category emits an explicit unsupported-scope row for mechanisms
that cannot be covered by its published static adapters, such as transient
systemd state, unmounted media, unsupported browser products, and application
plugin systems other than LibreOffice/OpenOffice.

## Command-line options

```text
autoruns [-a <*|blnsthkio|named[,named...]>] [-c|-ct|--json|-x] [-h] [-t] [-o <output file>] [--root <path>] [-nobanner]
```

Category selectors:

- `*`: all implemented Linux categories
- `b`: boot hooks
- `h`: image hijacks and preload hooks
- `i`: browser integrations
- `k`: dynamic loader hooks
- `l`: logon startups, the default
- `n`: network hooks
- `o`: application integrations
- `s`: services and module startup entries
- `t`: scheduled tasks

Use the named `device`, `mount`, or `device-mount` selector for device and mount
events. The short `d` selector retains its Windows-category compatibility
meaning and is not silently remapped.

Unsupported trust and VirusTotal options (`-m`, `-s`, `-u`, and `-v`) fail
closed with exit code 2. Unsupported Windows-only categories and static-adapter
boundaries are reported explicitly rather than mapped inaccurately.

## Scan safety and exit status

- `--root` is validated and canonicalized before scanning. On Linux, alternate
	roots use descriptor-relative `openat2` access with `RESOLVE_IN_ROOT` and
	`RESOLVE_NO_MAGICLINKS`. If that kernel facility is unavailable, the scanner
	reports a partial-scan diagnostic and requires the mounted image to remain
	immutable while the pathname fallback is used.
- Read, metadata, archive, and structured-parse failures are retained as
	diagnostics while valid rows are still emitted.
- Text sources and inspected XPI/OXT metadata are read under explicit per-file,
	member-count, per-member, and cumulative decompression limits.
- Target-file metadata is inspected without executing discovered code. Runtime
	service/process state (`active`, `inactive`, or `failed`) is not queried and
	is unavailable for offline roots.
- Exit code `0` means the selected static adapters completed, `1` is an output
	failure, `2` is an argument/root error, and `3` means partial scan results.
- `-h` computes SHA-256 in process. Table text escapes terminal controls,
	CSV/TSV cells are protected against spreadsheet formulas, and `-o` creates
	owner-only (`0600`) reports on Unix. Symlinked report destinations are
	rejected atomically. Per-target hash failures are partial-scan diagnostics.

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
