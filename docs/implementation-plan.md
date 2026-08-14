# Autoruns for Linux implementation plan

Last updated: 2026-08-14

## Current repository state

- The upstream GitHub repository `microsoft/Autoruns-for-Linux` has been unarchived and can accept pushes or pull requests subject to normal repository permissions and branch policies.
- The local clone is on the `initial-rust-implementation` feature branch. `origin` is `https://github.com/chakrik73/Autoruns-for-Linux.git` and `upstream` is `https://github.com/microsoft/Autoruns-for-Linux.git`.
- Intended developer workflow:
  1. Fork `microsoft/Autoruns-for-Linux` to `chakrik73/Autoruns-for-Linux`.
  2. Clone the fork or repoint this clone so `origin` is the fork and `upstream` is Microsoft.
  3. Create a feature branch from `main`.
  4. Push work to the fork and open a pull request to upstream.

If repository policy allows direct branch pushes, this existing clone can also push the local feature branch directly to upstream:

```powershell
git push -u origin initial-rust-implementation
```

Completed local remote setup for this existing clone:

```powershell
git remote rename origin upstream
git remote add origin https://github.com/chakrik73/Autoruns-for-Linux.git
```

## Reference inputs

### Windows AutoRuns comparison target

Reference tree: `C:\develop\AutoRuns`

Observed Windows CLI/core shape:

- `autoruns-cli/Options.cpp` defines the command-line surface: `-a`, `-c`, `-ct`, `-h`, `-m`, `-o`, `-s`, `-t`, `-u`, `-v[rs]`, `-vt`, `-x`, `-z`, `-nobanner`, plus an optional user selector.
- Default category is logon startups (`-a l`).
- `autoruns-cli/Scanner.cpp` maps category bits to scanner functions and scans in GUI-compatible category order.
- `AutorunsCore/AutorunsCore.h` defines the central entry model: name, description, publisher, image path, additional path, timestamp, VirusTotal fields, key/value name, and flags such as title, missing file, verified, unchecked, link, and disabled/deleted/new comparison states.

Windows categories to compare against:

| Windows flag | Windows category | Linux equivalent plan |
| --- | --- | --- |
| `l` | Logon startups | XDG autostart `.desktop`, shell profile files, cron user entries, systemd user units |
| `s` | Services and drivers | systemd system services, SysV init scripts, kernel modules and module-load config |
| `t` | Scheduled tasks | system and user cron, anacron, systemd timers |
| `m` | WMI entries | No direct equivalent; document as unsupported/not applicable |
| `e` | Explorer addons | Desktop environment extension/autostart hooks where detectable |
| `i` | Internet Explorer addons | Browser extension/native messaging autostart surfaces where applicable, lower priority |
| `k` | Known DLLs | `ld.so.preload`, dynamic linker config, loader paths |
| `h` | Image hijacks | shell command hijacks, PATH shadowing, loader preload, alternatives hooks |
| `b` | Boot execute | initramfs/dracut hooks, rc.local, boot loader/init hooks |
| `n` | Winsock/network providers | NetworkManager dispatcher scripts, if-up/down hooks, systemd network hooks |
| `o` | Office addins | LibreOffice/OpenOffice extension startup surfaces, lower priority |
| Other Windows-only flags | AppInit, Winlogon, LSA, Print monitors, Packaged apps, Sidebar | Mark unsupported unless a clear Linux persistence/autostart equivalent exists |

### Sysinternals-jcd cues

Reference repo: `https://github.com/microsoft/Sysinternals-jcd`

Useful patterns:

- Rust/Cargo project structure with `Cargo.toml`, `src/main.rs`, and shell-based tests.
- Development docs that explain build/test/package flows.
- Linux packaging helper patterns for deb/rpm/brew in `makePackages.sh`.
- Tests use shell and Python smoke/regression scripts around the compiled binary.
- Existing Sysinternals pipeline pattern can extend shared pipeline templates; local AutoRuns Windows pipeline uses `templates/sysinternals.yml@templates` with a build/sign job.

## Product goals

1. Provide a Linux command-line Autoruns tool that lists programs and scripts configured to run automatically during boot, login, scheduled execution, service startup, or common persistence hooks.
2. Preserve the familiar Autorunsc-style command-line feel where practical.
3. Emit script-friendly output in table, CSV, TSV, and JSON formats. XML can be added later for closer Windows parity.
4. Keep scanner modules independent, testable, and conservative: report evidence paths and parsed commands without executing discovered entries.
5. Make unsupported Windows-only concepts explicit in output/status instead of pretending there is a one-to-one mapping.

## Initial Rust architecture

```text
autoruns-for-linux/
├── Cargo.toml
├── src/
│   ├── main.rs              # CLI entry point
│   ├── cli.rs               # argument parsing and category selection
│   ├── model.rs             # AutorunEntry, Category, EntryStatus
│   ├── output.rs            # table/csv/tsv/json writers
│   └── scanners/
│       ├── mod.rs           # scanner trait and dispatch
│       ├── desktop.rs       # XDG autostart
│       ├── systemd.rs       # system/user services and timers
│       ├── cron.rs          # cron/anacron surfaces
│       ├── shell.rs         # shell profile startup hooks
│       └── linux.rs         # miscellaneous loader/network/boot hooks
├── tests/
│   └── smoke.rs             # CLI smoke tests using temporary fixtures
├── docs/
│   └── implementation-plan.md
└── azure-pipelines.yml      # future shared Sysinternals pipeline integration
```

## CLI plan

Initial supported flags:

- `-a <selectors>`: category selection. `*` means all. Default is logon.
- `-c`: CSV output.
- `-ct`: tab-delimited output.
- `--json`: JSON output.
- `-o <path>`: write output to a file.
- `-h`: include file hashes when the target file can be resolved.
- `-m`: hide Microsoft-signed entries. Initially accepted but a no-op until Linux signature/publisher logic exists.
- `-s`: verify signatures. Initially accepted but reports unsupported until implemented.
- `-t`: normalize timestamps to UTC where metadata exists.
- `-nobanner`: suppress banner.
- `--root <path>`: scan an alternate root for tests, containers, mounted systems, and offline images.

Initial Linux selector mapping:

- `l`: logon-related startup entries.
- `s`: services and kernel/module startup entries.
- `t`: scheduled tasks.
- `b`: boot hooks.
- `h`: hijack/preload hooks.
- `n`: network hooks.
- `*`: all implemented Linux categories.

## Implementation phases

### Phase 0: bootstrap

- Add Cargo project metadata.
- Add CLI parsing, shared model, output writers, scanner trait, and a small smoke test.
- Update README from template to build/run guidance.

### Phase 1: real local scanners

- XDG autostart scanner:
  - `/etc/xdg/autostart/*.desktop`
  - `$HOME/.config/autostart/*.desktop`
  - Parse `Name`, `Exec`, `Hidden`, `NoDisplay`, `OnlyShowIn`, `NotShowIn`, and comments.
- systemd scanner:
  - `/etc/systemd/system`, `/usr/lib/systemd/system`, `/lib/systemd/system`
  - user units under `$HOME/.config/systemd/user`
  - Parse `ExecStart`, `ExecStartPre`, `ExecStartPost`, `WantedBy`, `Also`, unit enabled state via symlinks where possible.
- cron scanner:
  - `/etc/crontab`, `/etc/cron.d`, `/etc/cron.hourly`, `/etc/cron.daily`, `/etc/cron.weekly`, `/etc/cron.monthly`
  - user crontabs where readable.

### Phase 2: parity and enrichment

- Hashing for resolved image paths.
- Publisher/signature strategy for Linux packages and signatures.
- Disabled state detection for systemd, desktop files, and Autoruns-disabled style quarantines if adopted.
- JSON schema and regression fixtures.

### Phase 3: packaging and CI

- Add Azure Pipelines YAML based on Sysinternals shared template expectations and Rust install/build/test steps.
- Add deb/rpm packaging scripts, then signing/publishing integration as directed by Sysinternals release requirements.

## Status

| Area | Status | Notes |
| --- | --- | --- |
| Repo/fork workflow | Ready | Upstream is unarchived. Fork remote is configured as `origin`; Microsoft repo is configured as `upstream`. |
| Local branch | Done | Created `initial-rust-implementation` locally from `main`; current changes are on that branch. |
| Rust toolchain | Done | Installed via rustup (cargo 1.97) with gcc/libc6-dev for linking in the Linux dev environment. |
| Planning | Done for initial slice | This document created from local Windows AutoRuns source and Sysinternals-jcd patterns. |
| Implementation | Done for initial slice | Cargo project, CLI parser, model, output writers, and Linux scanners for XDG autostart, shell startup, systemd services/timers, cron, boot hooks, module load config, loader hooks, and network hooks. |
| Validation | Done | `cargo build`, `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, and `cargo test` all pass. Added `tests/smoke.rs` with fixture-based `--root` integration tests (9 tests total). |
| CI pipeline | Done for initial slice | Added `azure-pipelines.yml` + `templates/build.yaml` following the Sysinternals-jcd pattern (shared repo resource + reusable build template, `ubuntu-24.04` pool, build/fmt/clippy/test steps). |

## Immediate next steps

1. Wire the `sysinternals` GitHub service connection / shared template repo in Azure DevOps so the pipeline resource resolves.
2. Add signature/publisher strategy for Linux package and binary trust metadata.
3. Add hashing/publisher enrichment (Phase 2) and JSON schema regression fixtures.
4. Add deb/rpm packaging scripts (Phase 3), reusing the jcd `makePackages.sh` approach.

Completed since last update:

- Installed the Rust toolchain in the Linux environment and confirmed the project builds.
- Ran `cargo fmt`, `cargo clippy --all-targets -- -D warnings`, `cargo test`, and `cargo run -- --help`.
- Added `tests/smoke.rs` with fixture-based `--root` integration tests covering every implemented scanner category and each output format.
- Added the Azure Pipelines definition and reusable build template modeled on Sysinternals-jcd.

## Code review fixes

Addressed on the `initial-rust-implementation` PR. All changes verified with `cargo fmt --all`, `cargo clippy --all-targets -- -D warnings`, and `cargo test --all` (9 tests).

First review round:

- **desktop**: Deduplicate autostart directories (`dirs.sort(); dirs.dedup()`) so a `$HOME` under `/home` is not scanned twice, preventing duplicate `.desktop` entries.
- **linux (network)**: Use the file `PathBuf` directly for the network-hook `image_path` instead of shell-parsing the path (which truncated paths containing whitespace).
- **cron (filter)**: Only skip environment-assignment lines by inspecting the first token, instead of dropping any cron line containing `=` (which discarded valid commands with `=` in their arguments).
- **cron (@macros)**: Strip the leading schedule macro and user field from system crontab (`/etc/crontab`, `/etc/cron.d`) `@reboot`/`@daily` entries so the command no longer includes the user column.
- **main (hashing)**: Re-anchor `image_path` under `--root` via `resolve_under_root` before hashing, so offline/mounted scans hash the in-image file rather than the host file.
- **systemd (enablement)**: Detect enablement via `*.wants` under `/etc/systemd/system` as well as the unit's own directory, so units shipped in `/usr/lib` and `/lib` are not misreported as `unknown`.
- **output (Source)**: Root-strip the `Source` column so output is stable and independent of where the scan root is mounted.
- **CI (build.yaml)**: Use safe `LD_LIBRARY_PATH` expansion (`${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}`) so the script does not fail under `set -u` when the variable is unset.

Second review round (suppressed/consistency notes):

- **image_path/command contract**: Store the in-image absolute path (e.g. `/etc/profile`) rather than the rooted host path for rc.local, SysV init scripts, network hooks, shell startup files, and run-parts cron scripts. Added the shared `in_root_path` helper (a no-op when scanning `/`); `source_path` remains rooted for reading.
- **first_command_path**: Skip leading `KEY=value` environment-assignment tokens (via new `shell_tokens`/`is_env_assignment`) so commands like `FOO=bar /usr/bin/app` still yield a usable `image_path` for hashing.
- **CI (build.yaml)**: Check for `rustup` (not `cargo`) before installing the toolchain, so images with a distro-provided `cargo` but no `rustup` do not fail the later `rustup component add` static-analysis step.

Deferred (needs a product decision):

- `-t` / `utc_timestamps` is currently a no-op. Gating timestamp visibility on it would change default output semantics, so it is left unchanged pending direction on the intended behavior.