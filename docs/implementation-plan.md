# Autoruns for Linux implementation plan

Last updated: 2026-08-12

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
| Rust toolchain | Blocked locally | `cargo` and `rustc` are not installed or not on PATH in this Windows environment. |
| Planning | Done for initial slice | This document created from local Windows AutoRuns source and Sysinternals-jcd patterns. |
| Implementation | In progress | Added initial Cargo project, CLI parser, model, output writers, and Linux scanners for XDG autostart, shell startup, systemd, cron, boot hooks, module load config, loader hooks, and network hooks. |
| Validation | Partial | Editor diagnostics report no errors and `git diff --check` passes. Cargo build/test blocked until Rust is installed. |

## Immediate next steps

1. Install Rust or run validation in a Linux build environment.
2. Run `cargo fmt`, `cargo test`, and `cargo run -- --help`.
3. Add fixture-based tests for `--root` scanner behavior.
4. Decide whether Azure Pipelines should use a plain Rust job or an internal Sysinternals shared template.
5. Add signature/publisher strategy for Linux package and binary trust metadata.