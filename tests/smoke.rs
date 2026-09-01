//! CLI smoke tests that build a temporary filesystem root with representative
//! autorun fixtures and assert that each scanner category and output format
//! behaves as expected when driven through the `--root` option.
//!
//! The fixtures rely on Unix symlinks, so the whole suite is gated to Unix
//! targets to keep `cargo test` compiling elsewhere.
#![cfg(unix)]

use std::collections::HashSet;
use std::fs;
use std::io::Write;
use std::os::unix::fs::{self as unix_fs, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};

const BIN: &str = env!("CARGO_BIN_EXE_autoruns");

static COUNTER: AtomicU32 = AtomicU32::new(0);

/// A temporary directory that is removed when it goes out of scope.
struct TempRoot {
    path: PathBuf,
}

impl TempRoot {
    fn new() -> Self {
        // Combine the PID, a high-resolution timestamp, and a process-local
        // counter so the path is unique across concurrent runs and re-runs, then
        // create it with `create_dir` (not `create_dir_all`) so an existing
        // leftover directory is a hard error rather than silently reused.
        let unique = format!(
            "autoruns-smoke-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|elapsed| elapsed.as_nanos())
                .unwrap_or(0),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        );
        let path = std::env::temp_dir().join(unique);
        fs::create_dir(&path).expect("create unique temp root");
        Self { path }
    }

    fn write(&self, relative: &str, contents: &str) {
        let full = self.path.join(relative);
        if let Some(parent) = full.parent() {
            fs::create_dir_all(parent).expect("create fixture parent");
        }
        fs::write(&full, contents).expect("write fixture");
    }

    fn write_zip(&self, relative: &str, members: &[(&str, &[u8])]) {
        let full = self.path.join(relative);
        if let Some(parent) = full.parent() {
            fs::create_dir_all(parent).expect("create ZIP fixture parent");
        }
        let file = fs::File::create(full).expect("create ZIP fixture");
        let mut archive = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        for (name, contents) in members {
            archive
                .start_file(*name, options)
                .expect("start ZIP fixture member");
            archive
                .write_all(contents)
                .expect("write ZIP fixture member");
        }
        archive.finish().expect("finish ZIP fixture");
    }

    fn write_empty_zip_members(&self, relative: &str, member_count: usize) {
        let full = self.path.join(relative);
        if let Some(parent) = full.parent() {
            fs::create_dir_all(parent).expect("create ZIP fixture parent");
        }
        let file = fs::File::create(full).expect("create ZIP fixture");
        let mut archive = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        for index in 0..member_count {
            archive
                .start_file(format!("member-{index:04}"), options)
                .expect("start empty ZIP fixture member");
        }
        archive.finish().expect("finish ZIP fixture");
    }

    fn symlink(&self, relative_target: &str, relative_link: &str) {
        let link = self.path.join(relative_link);
        if let Some(parent) = link.parent() {
            fs::create_dir_all(parent).expect("create symlink parent");
        }
        // Use an absolute *in-image* target (leading `/`), like a real symlink
        // inside a scanned root, so the fixtures exercise root-aware resolution
        // rather than accidentally pointing at host paths under the temp dir.
        unix_fs::symlink(Path::new("/").join(relative_target), link).expect("create symlink");
    }

    fn path(&self) -> &Path {
        &self.path
    }

    fn set_mode(&self, relative: &str, mode: u32) {
        fs::set_permissions(self.path.join(relative), fs::Permissions::from_mode(mode))
            .expect("set fixture mode");
    }
}

impl Drop for TempRoot {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

/// Populate a temporary root with one fixture per implemented scanner.
fn populate(root: &TempRoot) {
    // XDG autostart (logon).
    root.write(
        "etc/xdg/autostart/example.desktop",
        "[Desktop Entry]\nType=Application\nName=Example Autostart\nComment=Example comment\nExec=/usr/bin/example --flag\n",
    );

    // Shell startup (logon).
    root.write("etc/profile", "# system profile\nexport PATH=$PATH\n");

    // systemd service (services) plus an enable symlink.
    root.write(
        "etc/systemd/system/example.service",
        "[Unit]\nDescription=Example Service\n\n[Service]\nExecStart=/usr/bin/exampled --serve\n\n[Install]\nWantedBy=multi-user.target\n",
    );
    root.symlink(
        "etc/systemd/system/example.service",
        "etc/systemd/system/multi-user.target.wants/example.service",
    );

    // Kernel module load config (services).
    root.write("etc/modules", "# modules\nmymodule\n");

    // systemd timer (scheduled tasks).
    root.write(
        "etc/systemd/system/backup.timer",
        "[Unit]\nDescription=Backup Timer\n\n[Timer]\nOnCalendar=daily\n\n[Install]\nWantedBy=timers.target\n",
    );

    // cron entries (scheduled tasks).
    root.write(
        "etc/crontab",
        "# system crontab\n17 * * * * root /usr/bin/cronjob --run\n@reboot root /usr/bin/bootjob\n",
    );
    root.write("etc/cron.daily/dailyjob", "#!/bin/sh\n/usr/bin/daily\n");
    root.set_mode("etc/cron.daily/dailyjob", 0o755);

    // Boot hooks.
    root.write("etc/rc.local", "#!/bin/sh\n/usr/local/bin/startup.sh\n");

    // Loader / known-DLL equivalents.
    root.write("etc/ld.so.preload", "/opt/lib/inject.so\n");

    // Network hooks.
    root.write(
        "etc/NetworkManager/dispatcher.d/50-hook",
        "#!/bin/sh\n/usr/bin/nethook\n",
    );
    root.set_mode("etc/NetworkManager/dispatcher.d/50-hook", 0o755);
}

fn run(args: &[&str]) -> String {
    let output = run_output(args);
    assert!(
        output.status.success(),
        "binary exited with failure: {:?}\nstderr: {}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("utf-8 stdout")
}

fn run_output(args: &[&str]) -> std::process::Output {
    Command::new(BIN)
        .args(args)
        .output()
        .expect("run autoruns binary")
}

#[test]
fn help_lists_usage() {
    let stdout = run(&["--help"]);
    assert!(stdout.contains("Usage: autoruns"));
    assert!(stdout.contains("-nobanner"));
}

#[test]
fn scans_all_categories_from_root() {
    let root = TempRoot::new();
    populate(&root);
    let root_arg = root.path().to_string_lossy().to_string();

    let stdout = run(&["-nobanner", "-a", "*", "--root", &root_arg]);

    // Logon.
    assert!(
        stdout.contains("Example Autostart"),
        "missing autostart entry:\n{stdout}"
    );
    assert!(stdout.contains("/usr/bin/example --flag"));
    assert!(
        stdout.contains("profile"),
        "missing shell startup entry:\n{stdout}"
    );

    // Services.
    assert!(stdout.contains("example.service"));
    assert!(stdout.contains("/usr/bin/exampled --serve"));
    assert!(stdout.contains("mymodule"));

    // Scheduled tasks.
    assert!(stdout.contains("backup.timer"));
    assert!(stdout.contains("/usr/bin/cronjob --run"));
    assert!(stdout.contains("dailyjob"));

    // Boot.
    assert!(stdout.contains("rc.local"));

    // Loader.
    assert!(stdout.contains("/opt/lib/inject.so"));

    // Network.
    assert!(stdout.contains("50-hook"));
}

#[test]
fn enabled_systemd_unit_is_reported_enabled() {
    let root = TempRoot::new();
    populate(&root);
    let root_arg = root.path().to_string_lossy().to_string();

    let stdout = run(&["-nobanner", "-a", "s", "--root", &root_arg, "-c"]);

    let enabled_line = stdout
        .lines()
        .find(|line| line.contains("example.service"))
        .expect("service line present");
    assert!(
        enabled_line.contains("enabled"),
        "service should be enabled via wants symlink: {enabled_line}"
    );
}

#[test]
fn registered_targets_report_presence_executability_and_resolution() {
    let root = TempRoot::new();
    root.write("usr/bin/present-daemon", "#!/bin/sh\nexit 0\n");
    root.set_mode("usr/bin/present-daemon", 0o755);
    root.write("usr/bin/non-executable-daemon", "not executable\n");
    root.symlink("usr/bin/inaccessible-daemon", "usr/bin/inaccessible-daemon");
    for (name, command) in [
        ("present.service", "/usr/bin/present-daemon --serve"),
        (
            "non-executable.service",
            "/usr/bin/non-executable-daemon --serve",
        ),
        ("missing.service", "/usr/bin/missing-daemon --serve"),
        (
            "inaccessible.service",
            "/usr/bin/inaccessible-daemon --serve",
        ),
        ("relative.service", "relative-daemon --serve"),
    ] {
        root.write(
            &format!("etc/systemd/system/{name}"),
            &format!("[Service]\nExecStart={command}\n"),
        );
        root.symlink(
            &format!("etc/systemd/system/{name}"),
            &format!("etc/systemd/system/multi-user.target.wants/{name}"),
        );
    }
    let root_arg = root.path().to_string_lossy().to_string();

    let output = run_output(&["-nobanner", "-a", "s", "--root", &root_arg, "--json"]);
    assert_eq!(output.status.code(), Some(3));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("inspect target"), "stderr: {stderr}");
    let stdout = String::from_utf8(output.stdout).expect("utf-8 stdout");
    let parsed: serde_json::Value = serde_json::from_str(&stdout).expect("valid JSON output");
    let entries = parsed.as_array().expect("top-level JSON array");
    let service = |name: &str| {
        entries
            .iter()
            .find(|entry| entry.get("name").and_then(serde_json::Value::as_str) == Some(name))
            .unwrap_or_else(|| panic!("missing service {name}: {stdout}"))
    };

    let present = service("present.service");
    assert_eq!(present["status"], "enabled");
    assert_eq!(present["targetState"], "present");
    assert_eq!(present["targetExists"], true);
    assert_eq!(present["targetExecutable"], true);

    let non_executable = service("non-executable.service");
    assert_eq!(non_executable["status"], "enabled");
    assert_eq!(non_executable["targetState"], "present");
    assert_eq!(non_executable["targetExists"], true);
    assert_eq!(non_executable["targetExecutable"], false);

    let missing = service("missing.service");
    assert_eq!(missing["status"], "enabled");
    assert_eq!(missing["targetState"], "missing");
    assert_eq!(missing["targetExists"], false);
    assert_eq!(missing["targetExecutable"], false);

    let inaccessible = service("inaccessible.service");
    assert_eq!(inaccessible["status"], "enabled");
    assert_eq!(inaccessible["targetState"], "inaccessible");
    assert!(inaccessible["targetExists"].is_null());
    assert!(inaccessible["targetExecutable"].is_null());

    let unresolved = service("relative.service");
    assert_eq!(unresolved["status"], "enabled");
    assert_eq!(unresolved["targetState"], "unresolved");
    assert!(unresolved["targetExists"].is_null());
    assert!(unresolved["targetExecutable"].is_null());
}

#[test]
fn systemd_applies_precedence_masks_dropins_and_usrmerge_deduplication() {
    let root = TempRoot::new();
    root.write(
        "usr/lib/systemd/system/choice.service",
        "[Service]\nExecStart=/usr/bin/lower\n",
    );
    root.write(
        "etc/systemd/system/choice.service",
        "[Service]\nExecStart=/usr/bin/higher\n",
    );
    root.write(
        "etc/systemd/system/choice.service.d/10-command.conf",
        "[Service]\nExecStart=\nExecStart=/usr/bin/dropin --flag\n",
    );
    root.write(
        "usr/lib/systemd/system/masked.service",
        "[Service]\nExecStart=/usr/bin/masked-payload\n",
    );
    root.symlink("dev/null", "etc/systemd/system/masked.service");
    root.symlink("usr/lib", "lib");
    let root_arg = root.path().to_string_lossy().to_string();

    let stdout = run(&["-nobanner", "-a", "s", "--root", &root_arg, "-c"]);

    let choice_rows: Vec<_> = stdout
        .lines()
        .filter(|line| line.contains(",choice.service,"))
        .collect();
    assert_eq!(
        choice_rows.len(),
        2,
        "effective plus shadowed only: {stdout}"
    );
    assert!(
        choice_rows
            .iter()
            .any(|line| line.contains("/usr/bin/dropin --flag") && !line.contains("shadowed")),
        "choice rows: {choice_rows:?}"
    );
    assert!(!stdout.contains("/usr/bin/lower"), "stdout: {stdout}");

    let masked = stdout
        .lines()
        .find(|line| line.contains(",masked.service,") && line.contains("/etc/systemd/system"))
        .expect("effective mask");
    assert!(masked.contains("disabled"), "mask: {masked}");
    assert!(masked.contains("masked by /dev/null"), "mask: {masked}");
}

#[test]
fn systemd_preserves_commands_instances_and_scope() {
    let root = TempRoot::new();
    root.write(
        "usr/lib/systemd/system/multi.service",
        "[Service]\nExecStartPre=/usr/bin/prep\nExecStart=/usr/bin/main \\\n          --continued\nExecStartPost=/usr/bin/post\n",
    );
    root.symlink(
        "usr/lib/systemd/system/multi.service",
        "etc/systemd/system/multi-user.target.wants/multi.service",
    );
    root.write(
        "usr/lib/systemd/system/worker@.service",
        "[Service]\nExecStart=/usr/bin/worker %i\n",
    );
    root.symlink(
        "usr/lib/systemd/system/worker@.service",
        "etc/systemd/system/multi-user.target.wants/worker@blue.service",
    );
    root.write(
        "usr/lib/systemd/system/collision.service",
        "[Service]\nExecStart=/usr/bin/system-collision\n",
    );
    root.write(
        "usr/lib/systemd/user/collision.service",
        "[Service]\nExecStart=/usr/bin/user-collision\n",
    );
    root.symlink(
        "usr/lib/systemd/system/collision.service",
        "etc/systemd/system/multi-user.target.wants/collision.service",
    );
    root.write(
        "usr/lib/systemd/system/condition.service",
        "[Unit]\nExecCondition=/usr/bin/not-a-service-command\n[Service]\nExecCondition=/usr/bin/condition\nExecStart=/usr/bin/condition-start\n",
    );
    let root_arg = root.path().to_string_lossy().to_string();

    let stdout = run(&["-nobanner", "-a", "s", "--root", &root_arg, "-c"]);

    assert!(stdout.contains("/usr/bin/prep"), "stdout: {stdout}");
    assert!(
        stdout.contains("/usr/bin/main --continued"),
        "stdout: {stdout}"
    );
    assert!(stdout.contains("/usr/bin/post"), "stdout: {stdout}");
    assert!(stdout.contains("/usr/bin/condition"), "stdout: {stdout}");
    assert!(
        stdout.contains("/usr/bin/condition-start"),
        "stdout: {stdout}"
    );
    assert!(
        !stdout.contains("not-a-service-command"),
        "stdout: {stdout}"
    );
    let multi_count = stdout
        .lines()
        .filter(|line| line.contains(",multi.service,"))
        .count();
    assert_eq!(multi_count, 3, "stdout: {stdout}");

    let instance = stdout
        .lines()
        .find(|line| line.contains(",worker@blue.service,"))
        .expect("enabled template instance");
    assert!(instance.contains("enabled"), "instance: {instance}");
    assert!(
        instance.contains("/usr/bin/worker blue"),
        "instance: {instance}"
    );

    let system = stdout
        .lines()
        .find(|line| line.contains("/usr/bin/system-collision"))
        .expect("system collision row");
    let user = stdout
        .lines()
        .find(|line| line.contains("/usr/bin/user-collision"))
        .expect("user collision row");
    assert!(system.contains("enabled"), "system row: {system}");
    assert!(!user.contains("enabled"), "user row: {user}");
}

#[test]
fn systemd_timer_and_path_units_resolve_payloads() {
    let root = TempRoot::new();
    root.write(
        "etc/systemd/system/payload.service",
        "[Service]\nExecCondition=/usr/bin/payload-condition\nExecStartPre=/usr/bin/payload-pre\nExecStart=/usr/bin/payload --run\nExecStartPost=/usr/bin/payload-post\n",
    );
    root.write(
        "etc/systemd/system/schedule.timer",
        "[Timer]\nOnCalendar=daily\nUnit=payload.service\n",
    );
    root.symlink(
        "etc/systemd/system/schedule.timer",
        "etc/systemd/system/timers.target.wants/schedule.timer",
    );
    root.write(
        "etc/systemd/system/watch.path",
        "[Path]\nPathModified=/watched\nUnit=payload.service\n",
    );
    for (extension, section) in [
        ("socket", "Socket"),
        ("device", "Unit"),
        ("mount", "Mount"),
        ("automount", "Automount"),
    ] {
        root.write(
            &format!("etc/systemd/system/payload.{extension}"),
            &format!("[{section}]\n"),
        );
    }
    let root_arg = root.path().to_string_lossy().to_string();

    let timers = run(&["-nobanner", "-a", "t", "--root", &root_arg, "-c"]);
    assert_systemd_trigger_phases(&timers, "schedule.timer");
    assert!(timers.contains("OnCalendar=daily"), "timers: {timers}");

    let services = run(&["-nobanner", "-a", "s", "--root", &root_arg, "-c"]);
    assert_systemd_trigger_phases(&services, "payload.socket");

    let devices = run(&["-nobanner", "-a", "device", "--root", &root_arg, "-c"]);
    assert_systemd_trigger_phases(&devices, "watch.path");
    assert!(
        devices.contains("PathModified=/watched"),
        "devices: {devices}"
    );
    for name in ["payload.device", "payload.mount", "payload.automount"] {
        assert_systemd_trigger_phases(&devices, name);
    }
}

fn assert_systemd_trigger_phases(output: &str, name: &str) {
    let rows: Vec<_> = output
        .lines()
        .filter(|line| line.contains(&format!(",{name},")))
        .collect();
    assert_eq!(rows.len(), 4, "{name} rows: {rows:?}\n{output}");
    for command in [
        "/usr/bin/payload-condition",
        "/usr/bin/payload-pre",
        "/usr/bin/payload --run",
        "/usr/bin/payload-post",
    ] {
        assert!(
            rows.iter().any(|row| row.contains(command)),
            "missing {command} for {name}: {rows:?}"
        );
    }
}

#[test]
fn browser_scanner_preserves_browser_profile_relationships() {
    let root = TempRoot::new();
    for (profile, state) in [("Default", 1), ("Profile 1", 0)] {
        root.write(
            &format!("home/alice/.config/chromium/{profile}/Preferences"),
            &"{\"extensions\":{\"settings\":{\"abcdefghijklmnop\":{\"state\":$STATE}}}}"
                .replace("$STATE", &state.to_string()),
        );
        root.write(
            &format!(
                "home/alice/.config/chromium/{profile}/Extensions/abcdefghijklmnop/1.2.3/manifest.json"
            ),
            "{\"name\":\"Fixture Extension\",\"version\":\"1.2.3\",\"manifest_version\":3}",
        );
    }
    root.write(
        "home/alice/.config/chromium/NativeMessagingHosts/com.example.host.json",
        "{\"name\":\"com.example.host\",\"description\":\"Fixture host\",\"path\":\"/opt/example/host\",\"allowed_origins\":[\"chrome-extension://abcdefghijklmnop/\"]}",
    );
    root.write(
        "etc/chromium/policies/managed/extensions.json",
        "{\"ExtensionInstallForcelist\":[\"forcedextension;https://example.invalid/update\"]}",
    );
    root.write(
        "home/alice/.local/share/applications/chromium-custom.desktop",
        "[Desktop Entry]\nType=Application\nExec=chromium --load-extension=/opt/unpacked-one,/opt/unpacked-two\n",
    );
    root.write(
        "home/alice/.mozilla/firefox/profiles.ini",
        "[Profile0]\nName=default\nIsRelative=1\nPath=fixture.default\n",
    );
    root.write(
        "home/alice/.mozilla/firefox/fixture.default/extensions.json",
        "{\"addons\":[{\"id\":\"firefox@example\",\"type\":\"extension\",\"active\":true,\"userDisabled\":false,\"appDisabled\":false,\"version\":\"2.0\",\"path\":\"/opt/firefox-addon.xpi\",\"defaultLocale\":{\"name\":\"Firefox Fixture\"}}]}",
    );
    root.write(
        "home/alice/.mozilla/firefox/fixture.default/pkcs11.txt",
        "name=Fixture Token\nlibrary=/opt/lib/libfixture-pkcs11.so\n",
    );
    let root_arg = root.path().to_string_lossy().to_string();

    let stdout = run(&["-nobanner", "-a", "i", "--root", &root_arg, "-c"]);

    let chromium_rows: Vec<_> = stdout
        .lines()
        .filter(|line| line.contains("Fixture Extension"))
        .collect();
    assert_eq!(chromium_rows.len(), 2, "stdout: {stdout}");
    assert!(
        chromium_rows
            .iter()
            .any(|line| line.contains("enabled") && line.contains("Chromium/Default")),
        "rows: {chromium_rows:?}"
    );
    assert!(
        chromium_rows
            .iter()
            .any(|line| line.contains("disabled") && line.contains("Chromium/Profile 1")),
        "rows: {chromium_rows:?}"
    );
    let host = stdout
        .lines()
        .find(|line| line.contains("com.example.host"))
        .expect("native messaging host");
    assert!(host.contains("conditional"), "host: {host}");
    assert!(
        host.contains("not assumed to start with the browser"),
        "host: {host}"
    );
    assert!(stdout.contains("forcedextension"), "stdout: {stdout}");
    assert!(stdout.contains("/opt/unpacked-one"), "stdout: {stdout}");
    assert!(stdout.contains("/opt/unpacked-two"), "stdout: {stdout}");
    assert!(stdout.contains("Firefox Fixture"), "stdout: {stdout}");
    assert!(
        stdout.contains("/opt/lib/libfixture-pkcs11.so"),
        "stdout: {stdout}"
    );
}

#[test]
fn malformed_browser_json_is_a_partial_scan() {
    let root = TempRoot::new();
    root.write(
        "home/alice/.config/chromium/Default/Preferences",
        "{not valid json",
    );
    let root_arg = root.path().to_string_lossy().to_string();

    let output = run_output(&["-nobanner", "-a", "browser", "--root", &root_arg, "-c"]);

    assert_eq!(output.status.code(), Some(3));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("parse JSON"), "stderr: {stderr}");
    assert!(stderr.contains("Preferences"), "stderr: {stderr}");
}

#[test]
fn device_scanner_reports_effective_static_activation_evidence() {
    let root = TempRoot::new();
    root.write(
        "usr/lib/udev/rules.d/50-storage.rules",
        "ACTION==\"add\", RUN{program}+=\"/usr/bin/lower-udev\"\n",
    );
    root.write(
        "etc/udev/rules.d/50-storage.rules",
        "ACTION==\"add\", SUBSYSTEM==\"block\", PROGRAM==\"/usr/bin/probe-device\", IMPORT{program}=\"/usr/bin/import-device\", RUN{program}+=\"/usr/bin/run-device --mount\", ENV{SYSTEMD_WANTS}+=\"mount-handler.service\"\n",
    );
    root.write(
        "usr/lib/udev/rules.d/60-masked.rules",
        "ACTION==\"add\", RUN{program}+=\"/usr/bin/masked-udev\"\n",
    );
    root.symlink("dev/null", "etc/udev/rules.d/60-masked.rules");
    root.write(
        "etc/fstab",
        "/dev/sdb1 /media/archive auto x-systemd.automount,x-systemd.requires=prepare.service 0 0\n",
    );
    root.write(
        "etc/auto.master",
        "/misc program:/usr/bin/autofs-map --timeout=60\n",
    );
    root.write("media/alice/USB/autorun.sh", "#!/bin/sh\nexit 0\n");
    root.set_mode("media/alice/USB/autorun.sh", 0o755);
    let root_arg = root.path().to_string_lossy().to_string();

    let stdout = run(&["-nobanner", "-a", "device", "--root", &root_arg, "-c"]);

    for expected in [
        "/usr/bin/probe-device",
        "/usr/bin/import-device",
        "/usr/bin/run-device --mount",
        "mount-handler.service",
        "x-systemd.automount",
        "x-systemd.requires=prepare.service",
        "/usr/bin/autofs-map",
        "/media/alice/USB/autorun.sh",
    ] {
        assert!(stdout.contains(expected), "missing {expected}: {stdout}");
    }
    assert!(!stdout.contains("lower-udev"), "stdout: {stdout}");
    assert!(!stdout.contains("masked-udev"), "stdout: {stdout}");
    let media = stdout
        .lines()
        .find(|line| line.contains("autorun.sh"))
        .expect("media evidence");
    assert!(media.contains("conditional"), "media: {media}");
    assert!(
        media.contains("was not mounted or executed"),
        "media: {media}"
    );
}

#[test]
fn media_markers_apply_precedence_and_validate_autoopen_targets() {
    let root = TempRoot::new();
    root.write("media/alice/valid/docs/readme.txt", "read me");
    root.write("media/alice/valid/docs/other.txt", "other");
    root.write(
        "media/alice/valid/.autoopen",
        "docs/readme.txt\r\nignored.txt",
    );
    root.write("media/alice/valid/autoopen", "docs/other.txt\n");

    root.write("media/alice/outside.txt", "outside");
    root.write("media/alice/traversal/.autoopen", "../outside.txt\n");

    root.write("media/alice/executable/run.sh", "#!/bin/sh\nexit 0\n");
    root.set_mode("media/alice/executable/run.sh", 0o755);
    root.write("media/alice/executable/autoopen", "run.sh\n");

    root.write("media/alice/precedence/.autorun", "#!/bin/sh\nexit 0\n");
    root.write("media/alice/precedence/autorun", "#!/bin/sh\nexit 0\n");
    root.write("media/alice/precedence/autorun.sh", "#!/bin/sh\nexit 0\n");
    for marker in [".autorun", "autorun", "autorun.sh"] {
        root.set_mode(&format!("media/alice/precedence/{marker}"), 0o755);
    }
    root.write("media/alice/precedence/document.txt", "document");
    root.write("media/alice/precedence/.autoopen", "document.txt\n");
    let root_arg = root.path().to_string_lossy().to_string();

    let stdout = run(&["-nobanner", "-a", "device", "--root", &root_arg, "-c"]);

    let valid = stdout
        .lines()
        .find(|line| line.contains(",.autoopen,") && line.contains("/valid/.autoopen"))
        .expect("effective .autoopen row");
    assert!(valid.contains(",conditional,"), "row: {valid}");
    assert!(
        valid.contains("/media/alice/valid/docs/readme.txt"),
        "row: {valid}"
    );
    assert!(!valid.contains("ignored.txt"), "row: {valid}");

    let lower_autoopen = stdout
        .lines()
        .find(|line| line.contains(",autoopen,") && line.contains("/valid/autoopen"))
        .expect("lower-priority autoopen row");
    assert!(
        lower_autoopen.contains(",shadowed,"),
        "row: {lower_autoopen}"
    );

    let traversal = stdout
        .lines()
        .find(|line| line.contains("/traversal/.autoopen"))
        .expect("traversal row");
    assert!(traversal.contains(",error,"), "row: {traversal}");
    assert!(traversal.contains("without '..'"), "row: {traversal}");

    let executable = stdout
        .lines()
        .find(|line| line.contains("/executable/autoopen"))
        .expect("executable autoopen row");
    assert!(executable.contains(",error,"), "row: {executable}");
    assert!(
        executable.contains("must be non-executable"),
        "row: {executable}"
    );
    assert!(
        executable.ends_with(",present,true,true"),
        "row: {executable}"
    );

    let selected_autorun = stdout
        .lines()
        .find(|line| line.contains(",.autorun,") && line.contains("/precedence/.autorun"))
        .expect("selected autorun row");
    assert!(
        selected_autorun.contains(",conditional,"),
        "row: {selected_autorun}"
    );
    for marker in ["autorun", "autorun.sh"] {
        let row = stdout
            .lines()
            .find(|line| {
                line.contains(&format!(",{marker},"))
                    && line.contains(&format!("/precedence/{marker}"))
            })
            .unwrap_or_else(|| panic!("missing {marker}: {stdout}"));
        assert!(row.contains(",shadowed,"), "row: {row}");
    }
    let conditional_autoopen = stdout
        .lines()
        .find(|line| line.contains("/precedence/.autoopen"))
        .expect("conditional autoopen row");
    assert!(
        conditional_autoopen.contains(",conditional,"),
        "row: {conditional_autoopen}"
    );
    assert!(
        conditional_autoopen.contains("policy ignores the selected autostart marker"),
        "row: {conditional_autoopen}"
    );
}

#[test]
fn application_scanner_reports_office_extensions_components_and_events() {
    let root = TempRoot::new();
    root.write(
        "usr/lib/libreoffice/share/extensions/system-fixture/description.xml",
        "<?xml version=\"1.0\"?><description identifier=\"com.example.system\" version=\"1.0\"><display-name><name>System Fixture</name></display-name></description>",
    );
    root.write(
        "usr/lib/libreoffice/share/extensions/system-fixture/fixture.components",
        "<?xml version=\"1.0\"?><components><component loader=\"com.sun.star.loader.SharedLibrary\" uri=\"vnd.sun.star.expand:$ORIGIN/libfixture.so\"><implementation name=\"com.example.Component\"/></component></components>",
    );
    root.write(
        "usr/lib/libreoffice/share/extensions/system-fixture/libfixture.so",
        "binary",
    );
    root.write(
        "home/alice/.config/libreoffice/4/user/registry/data/org/openoffice/Office/Events.xcu",
        "<?xml version=\"1.0\"?><oor:component-data xmlns:oor=\"http://openoffice.org/2001/registry\"><node oor:name=\"GlobalEventBroadcaster\"><node oor:name=\"OnStartApp\"><prop><value>macro:///Fixture.Start</value></prop></node></node></oor:component-data>",
    );
    root.write(
        "home/alice/.config/libreoffice/4/user/basic/Standard/script.xlb",
        "<?xml version=\"1.0\"?><library:library xmlns:library=\"http://openoffice.org/2000/library\" library:name=\"Standard\"/>",
    );
    let root_arg = root.path().to_string_lossy().to_string();

    let stdout = run(&["-nobanner", "-a", "applications", "--root", &root_arg, "-c"]);

    for expected in [
        "System Fixture",
        "vnd.sun.star.expand:$ORIGIN/libfixture.so",
        "/usr/lib/libreoffice/share/extensions/system-fixture/libfixture.so",
        "OnStartApp",
        "macro:///Fixture.Start",
        "script.xlb",
    ] {
        assert!(stdout.contains(expected), "missing {expected}: {stdout}");
    }
    let user_event = stdout
        .lines()
        .find(|line| line.contains("Events.xcu"))
        .expect("user event row");
    assert!(user_event.contains(",alice,"), "event: {user_event}");
    assert!(user_event.contains("conditional"), "event: {user_event}");
    assert!(
        stdout.contains("supported LibreOffice/OpenOffice"),
        "stdout: {stdout}"
    );
}

#[test]
fn malformed_office_xml_is_a_partial_scan() {
    let root = TempRoot::new();
    root.write(
        "usr/lib/libreoffice/share/extensions/broken/description.xml",
        "<description><unclosed>",
    );
    let root_arg = root.path().to_string_lossy().to_string();

    let output = run_output(&["-nobanner", "-a", "applications", "--root", &root_arg, "-c"]);

    assert_eq!(output.status.code(), Some(3));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("parse XML"), "stderr: {stderr}");
    assert!(stderr.contains("description.xml"), "stderr: {stderr}");
}

#[test]
fn application_scanner_parses_oxt_archives() {
    let root = TempRoot::new();
    root.write_zip(
        "usr/lib/libreoffice/share/extensions/fixture.oxt",
        &[
            (
                "description.xml",
                br#"<?xml version="1.0"?><description identifier="com.example.oxt" version="2.0"><display-name><name>OXT Fixture</name></display-name></description>"#,
            ),
            (
                "fixture.components",
                br#"<?xml version="1.0"?><components><component uri="vnd.sun.star.expand:$ORIGIN/lib/liboxt.so"><implementation name="com.example.OxtComponent"/></component></components>"#,
            ),
            (
                "Events.xcu",
                br#"<?xml version="1.0"?><node name="OnLoad"><value>macro:///Fixture.OnLoad</value></node>"#,
            ),
            ("lib/liboxt.so", b"fixture native helper"),
        ],
    );
    let root_arg = root.path().to_string_lossy().to_string();

    let stdout = run(&["-nobanner", "-a", "applications", "--root", &root_arg, "-c"]);

    for expected in [
        "OXT Fixture",
        "com.example.OxtComponent",
        "vnd.sun.star.expand:$ORIGIN/lib/liboxt.so",
        "OnLoad",
        "macro:///Fixture.OnLoad",
        "lib/liboxt.so",
        "fixture.oxt!",
    ] {
        assert!(stdout.contains(expected), "missing {expected}: {stdout}");
    }
}

#[test]
fn malformed_oxt_is_a_partial_scan() {
    let root = TempRoot::new();
    root.write(
        "usr/lib/libreoffice/share/extensions/broken.oxt",
        "not a ZIP archive",
    );
    let root_arg = root.path().to_string_lossy().to_string();

    let output = run_output(&["-nobanner", "-a", "applications", "--root", &root_arg, "-c"]);

    assert_eq!(output.status.code(), Some(3));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("parse OXT archive"), "stderr: {stderr}");
    assert!(stderr.contains("broken.oxt"), "stderr: {stderr}");
}

#[test]
fn oversized_text_and_archive_inputs_are_bounded() {
    const MIB: usize = 1024 * 1024;

    let root = TempRoot::new();
    root.write(
        "etc/xdg/autostart/oversized.desktop",
        &"x".repeat(16 * MIB + 1),
    );
    root.write(
        "home/alice/.mozilla/firefox/profiles.ini",
        "[Profile0]\nName=default\nIsRelative=1\nPath=fixture.default\n",
    );
    let oversized_member = vec![b' '; 8 * MIB + 1];
    root.write_zip(
        "home/alice/.mozilla/firefox/fixture.default/extensions/oversized.xpi",
        &[("manifest.json", &oversized_member)],
    );
    let cumulative_member = vec![b' '; 8 * MIB];
    root.write_zip(
        "usr/lib/libreoffice/share/extensions/cumulative.oxt",
        &[
            ("one.Events.xcu", &cumulative_member),
            ("two.Events.xcu", &cumulative_member),
            ("three.Events.xcu", &cumulative_member),
            ("four.Events.xcu", &cumulative_member),
            ("five.Events.xcu", &cumulative_member),
        ],
    );
    root.write_empty_zip_members(
        "usr/lib/libreoffice/share/extensions/many-members.oxt",
        1025,
    );
    root.write(
        "usr/lib/libreoffice/share/extensions/oversized-archive.oxt",
        "",
    );
    fs::OpenOptions::new()
        .write(true)
        .open(
            root.path()
                .join("usr/lib/libreoffice/share/extensions/oversized-archive.oxt"),
        )
        .expect("open sparse oversized archive")
        .set_len((128 * MIB + 1) as u64)
        .expect("size sparse oversized archive");
    let root_arg = root.path().to_string_lossy().to_string();

    let output = run_output(&["-nobanner", "-a", "*", "--root", &root_arg, "-c"]);

    assert_eq!(output.status.code(), Some(3));
    let stderr = String::from_utf8_lossy(&output.stderr);
    for (path, diagnostic) in [
        ("oversized.desktop", "text input size"),
        ("oversized.xpi", "archive member size"),
        ("cumulative.oxt", "archive content size"),
        ("many-members.oxt", "archive member count"),
        ("oversized-archive.oxt", "archive file size"),
    ] {
        assert!(stderr.contains(path), "missing path {path}: {stderr}");
        assert!(
            stderr.contains(diagnostic),
            "missing diagnostic {diagnostic}: {stderr}"
        );
    }
}

#[test]
fn all_scan_reports_unsupported_mechanism_boundaries() {
    let root = TempRoot::new();
    let root_arg = root.path().to_string_lossy().to_string();

    let stdout = run(&["-nobanner", "-a", "*", "--root", &root_arg, "-c"]);

    for expected in [
        "Unsupported logon adapters",
        "transient/generated runtime units",
        "bootloader, kernel command line, initramfs",
        "unsupported browser products",
        "unmounted media",
        "application plugin systems other than LibreOffice/OpenOffice",
        "outside the supported static adapter set",
    ] {
        assert!(stdout.contains(expected), "missing {expected}: {stdout}");
    }
}

#[test]
fn symlinked_wants_directory_reports_enabled_unit() {
    // A `*.wants` directory that is itself an absolute in-image symlink must be
    // discovered by list_dirs and listed under --root (not followed to the
    // host), so the unit it enables is still reported enabled.
    let root = TempRoot::new();
    root.write(
        "etc/systemd/system/linked.service",
        "[Unit]\nDescription=Linked Service\n\n[Service]\nExecStart=/usr/bin/linked --serve\n\n[Install]\nWantedBy=multi-user.target\n",
    );
    // The real wants directory lives at /realwants and holds the enablement
    // symlink for linked.service.
    root.symlink(
        "etc/systemd/system/linked.service",
        "realwants/linked.service",
    );
    // /etc/systemd/system/multi-user.target.wants -> /realwants (absolute).
    let link = root
        .path()
        .join("etc/systemd/system/multi-user.target.wants");
    fs::create_dir_all(link.parent().expect("link parent")).expect("create link parent");
    unix_fs::symlink("/realwants", &link).expect("create absolute wants symlink");
    let root_arg = root.path().to_string_lossy().to_string();

    let stdout = run(&["-nobanner", "-a", "s", "--root", &root_arg, "-c"]);

    let line = stdout
        .lines()
        .find(|line| line.contains("linked.service"))
        .expect("service line present");
    assert!(
        line.contains("enabled"),
        "unit enabled via a symlinked wants directory should be reported enabled: {line}"
    );
}

#[test]
fn intermediate_symlinked_component_is_reanchored_under_root() {
    // A symlink in an *intermediate* path component (not just the final one),
    // pointing at an absolute in-image path, must be re-anchored under --root so
    // the read stays inside the image instead of following the link to the host.
    let root = TempRoot::new();
    root.write("realetc/ld.so.preload", "/opt/lib/viaintermediate.so\n");
    // /etc -> /realetc (absolute, in-image). The scanned file /etc/ld.so.preload
    // therefore traverses a symlinked directory component.
    let link = root.path().join("etc");
    fs::create_dir_all(link.parent().expect("link parent")).expect("create link parent");
    unix_fs::symlink("/realetc", &link).expect("create absolute component symlink");
    let root_arg = root.path().to_string_lossy().to_string();

    let stdout = run(&["-nobanner", "-a", "k", "--root", &root_arg]);

    assert!(
        stdout.contains("viaintermediate.so"),
        "symlinked intermediate component should be re-anchored under --root:\n{stdout}"
    );
}

#[test]
fn absolute_symlink_is_reanchored_under_root() {
    // A scanned config that is an absolute symlink inside the image must be read
    // from the in-image target, never followed out to the host filesystem.
    let root = TempRoot::new();
    root.write("payload/preload.conf", "/opt/lib/reanchored.so\n");
    // Point /etc/ld.so.preload at an absolute in-image path. Without root-aware
    // resolution this would resolve against the host `/payload/preload.conf`.
    let link = root.path().join("etc/ld.so.preload");
    fs::create_dir_all(link.parent().expect("link parent")).expect("create link parent");
    unix_fs::symlink("/payload/preload.conf", &link).expect("create absolute symlink");
    let root_arg = root.path().to_string_lossy().to_string();

    let stdout = run(&["-nobanner", "-a", "k", "--root", &root_arg]);

    assert!(
        stdout.contains("reanchored.so"),
        "absolute symlink should be re-anchored under --root:\n{stdout}"
    );
}

#[test]
fn relative_dotdot_symlink_is_clamped_to_root() {
    // A relative symlink whose `..` components would climb above the image root
    // must be clamped inside the image, never resolved onto the host.
    let root = TempRoot::new();
    root.write("etc/payload.conf", "/opt/lib/clamped.so\n");
    // `/etc/ld.so.preload` -> ../../../../etc/payload.conf. Without clamping this
    // re-anchors to the host `/etc/payload.conf`; with clamping it stays in-image.
    let link = root.path().join("etc/ld.so.preload");
    fs::create_dir_all(link.parent().expect("link parent")).expect("create link parent");
    unix_fs::symlink("../../../../etc/payload.conf", &link).expect("create relative symlink");
    let root_arg = root.path().to_string_lossy().to_string();

    let stdout = run(&["-nobanner", "-a", "k", "--root", &root_arg]);

    assert!(
        stdout.contains("clamped.so"),
        "relative `..` symlink should be clamped to --root:\n{stdout}"
    );
}

#[test]
fn symlinked_directory_is_resolved_under_root() {
    // A scanned directory that is an absolute symlink inside the image (like a
    // usrmerge /lib -> /usr/lib) must be listed from its in-image target, not
    // followed out to the host filesystem.
    let root = TempRoot::new();
    root.write(
        "realcrond/dirjob",
        "* * * * * root /usr/bin/dircronjob --run\n",
    );
    // /etc/cron.d -> /realcrond (absolute, in-image).
    let link = root.path().join("etc/cron.d");
    fs::create_dir_all(link.parent().expect("link parent")).expect("create link parent");
    unix_fs::symlink("/realcrond", &link).expect("create absolute dir symlink");
    let root_arg = root.path().to_string_lossy().to_string();

    let stdout = run(&["-nobanner", "-a", "t", "--root", &root_arg, "-c"]);

    assert!(
        stdout.contains("/usr/bin/dircronjob --run"),
        "symlinked scan directory should be resolved under --root:\n{stdout}"
    );
    // The reported location should be the canonical scanned path, not the
    // symlink target it resolves to.
    assert!(
        stdout.contains("/etc/cron.d/dirjob") && !stdout.contains("/realcrond"),
        "location should be the canonical path, not the resolved target:\n{stdout}"
    );
}

#[test]
fn root_account_startup_files_are_scanned() {
    // The root account's home is /root, not under /home, so it must be scanned
    // explicitly -- important for offline image scans where $HOME is irrelevant.
    let root = TempRoot::new();
    root.write("root/.profile", "# root profile\nexport PATH=$PATH\n");
    let root_arg = root.path().to_string_lossy().to_string();

    let stdout = run(&["-nobanner", "-a", "l", "--root", &root_arg, "-c"]);

    assert!(
        stdout.contains("/root/.profile"),
        "root account startup files should be scanned:\n{stdout}"
    );
}

#[test]
fn default_category_is_logon_only() {
    let root = TempRoot::new();
    populate(&root);
    let root_arg = root.path().to_string_lossy().to_string();

    let stdout = run(&["-nobanner", "--root", &root_arg]);

    assert!(stdout.contains("Example Autostart"));
    // Services / boot fixtures must not appear when only logon is scanned.
    assert!(
        !stdout.contains("example.service"),
        "unexpected service entry:\n{stdout}"
    );
    assert!(
        !stdout.contains("/opt/lib/inject.so"),
        "unexpected loader entry:\n{stdout}"
    );
}

#[test]
fn xdg_user_override_masks_system_entry() {
    let root = TempRoot::new();
    root.write(
        "etc/xdg/autostart/app.desktop",
        "[Desktop Entry]\nType=Application\nName=System App\nExec=/usr/bin/system-app\n",
    );
    root.write(
        "home/alice/.config/autostart/app.desktop",
        "[Desktop Entry]\nType=Application\nName=User Mask\nHidden=true\n",
    );
    root.write(
        "etc/xdg/autostart/missing.desktop",
        "[Desktop Entry]\nType=Application\nName=Missing TryExec\nTryExec=/usr/bin/missing\nExec=/usr/bin/missing\n",
    );
    root.write(
        "etc/xdg/autostart/not-application.desktop",
        "[Desktop Entry]\nType=Link\nName=Not An Application\nExec=/usr/bin/should-not-run\n",
    );
    let root_arg = root.path().to_string_lossy().to_string();

    let stdout = run(&["-nobanner", "-a", "l", "--root", &root_arg, "-c"]);

    let user = stdout
        .lines()
        .find(|line| line.contains("User Mask"))
        .expect("user mask entry");
    assert!(user.contains("disabled"), "user entry: {user}");
    assert!(user.contains(",alice,"), "user entry: {user}");

    let system = stdout
        .lines()
        .find(|line| line.contains("System App"))
        .expect("shadowed system entry");
    assert!(system.contains("shadowed"), "system entry: {system}");
    assert!(!system.contains(",enabled,"), "system entry: {system}");

    let try_exec = stdout
        .lines()
        .find(|line| line.contains("Missing TryExec"))
        .expect("TryExec entry");
    assert!(try_exec.contains("disabled"), "TryExec entry: {try_exec}");

    let wrong_type = stdout
        .lines()
        .find(|line| line.contains("Not An Application"))
        .expect("non-Application entry");
    assert!(wrong_type.contains(",error,"), "entry: {wrong_type}");
    assert!(
        wrong_type.contains("Type is not Application"),
        "entry: {wrong_type}"
    );
}

#[test]
fn offline_xdg_status_ignores_invoking_session_environment() {
    let root = TempRoot::new();
    root.write(
        "home/alice/.config/autostart/desktop.desktop",
        "[Desktop Entry]\nType=Application\nName=Offline Desktop\nOnlyShowIn=KDE;\nExec=/usr/bin/desktop-app\n",
    );
    root.write(
        "home/alice/.config/autostart/tryexec.desktop",
        "[Desktop Entry]\nType=Application\nName=Offline TryExec\nTryExec=profile-helper\nExec=/usr/bin/profile-app\n",
    );
    root.write(
        "home/alice/custom-bin/profile-helper",
        "#!/bin/sh\nexit 0\n",
    );
    root.set_mode("home/alice/custom-bin/profile-helper", 0o755);
    let root_arg = root.path().to_string_lossy().to_string();
    let hostile_path = root.path().join("home/alice/custom-bin");

    let output = Command::new(BIN)
        .args(["-nobanner", "-a", "l", "--root", &root_arg, "-c"])
        .env("USER", "alice")
        .env("XDG_CURRENT_DESKTOP", "GNOME")
        .env("PATH", hostile_path)
        .output()
        .expect("run autoruns binary");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("utf-8 stdout");
    for name in ["Offline Desktop", "Offline TryExec"] {
        let row = stdout
            .lines()
            .find(|line| line.contains(name))
            .unwrap_or_else(|| panic!("missing {name}: {stdout}"));
        assert!(row.contains(",conditional,"), "row: {row}");
        assert!(!row.contains(",disabled,"), "row: {row}");
    }
}

#[test]
fn loader_expands_in_root_include_globs_without_cycles() {
    let root = TempRoot::new();
    root.write(
        "etc/ld.so.conf",
        "include /opt/vendor/ld/*.cfg\n/usr/local/lib\n",
    );
    root.write(
        "opt/vendor/ld/custom.cfg",
        "include /etc/ld.so.conf\n/opt/vendor/lib\n",
    );
    let root_arg = root.path().to_string_lossy().to_string();

    let stdout = run(&["-nobanner", "-a", "loader", "--root", &root_arg, "-c"]);

    assert!(stdout.contains("/opt/vendor/ld/*.cfg"), "stdout: {stdout}");
    assert!(
        stdout.contains("matched 1 in-root configuration file"),
        "stdout: {stdout}"
    );
    assert!(stdout.contains("/usr/local/lib"), "stdout: {stdout}");
    assert!(stdout.contains("/opt/vendor/lib"), "stdout: {stdout}");
    assert!(
        stdout.contains("/opt/vendor/ld/custom.cfg"),
        "stdout: {stdout}"
    );
}

#[test]
fn scheduled_tasks_enforce_eligibility_and_include_user_sources() {
    let root = TempRoot::new();
    root.write(
        "var/spool/cron/crontabs/alice",
        "5 4 * * * /usr/bin/user-job --run\n",
    );
    root.write("etc/anacrontab", "1 5 backup /usr/bin/anacron-job --run\n");
    root.write(
        "etc/cron.d/ignored.with-dot",
        "* * * * * root /usr/bin/ignored-crond\n",
    );
    root.write("etc/cron.daily/not-executable", "#!/bin/sh\nexit 0\n");
    root.write("etc/cron.daily/ignored.name", "#!/bin/sh\nexit 0\n");
    root.set_mode("etc/cron.daily/ignored.name", 0o755);
    root.write("etc/cron.daily/eligible-job", "#!/bin/sh\nexit 0\n");
    root.set_mode("etc/cron.daily/eligible-job", 0o755);
    let root_arg = root.path().to_string_lossy().to_string();

    let stdout = run(&["-nobanner", "-a", "t", "--root", &root_arg, "-c"]);

    let user_job = stdout
        .lines()
        .find(|line| line.contains("/usr/bin/user-job --run"))
        .expect("user crontab job");
    assert!(user_job.contains("5 4 * * *"), "user job: {user_job}");
    assert!(user_job.contains(",alice,"), "user job: {user_job}");
    assert!(
        stdout.contains("/usr/bin/anacron-job --run"),
        "stdout: {stdout}"
    );
    assert!(stdout.contains("eligible-job"), "stdout: {stdout}");
    assert!(!stdout.contains("ignored-crond"), "stdout: {stdout}");
    assert!(!stdout.contains("not-executable"), "stdout: {stdout}");
    assert!(!stdout.contains("ignored.name"), "stdout: {stdout}");
}

#[test]
fn modules_loader_and_alternatives_report_effective_targets() {
    let root = TempRoot::new();
    root.write("usr/lib/modules-load.d/10-choice.conf", "lower_module\n");
    root.write("etc/modules-load.d/10-choice.conf", "higher_module\n");
    root.write("usr/lib/modules-load.d/20-masked.conf", "masked_module\n");
    root.symlink("dev/null", "etc/modules-load.d/20-masked.conf");
    root.write("etc/modules-load.d/README", "ignored_module\n");
    root.write(
        "etc/modules-load.d/30-comment.conf",
        "; ignored comment\nkept_module ; trailing comment\n",
    );
    root.symlink("usr/lib", "lib");
    root.write(
        "etc/ld.so.preload",
        "/opt/lib/first.so /opt/lib/second.so\n",
    );
    root.write("usr/bin/tool.real", "binary");
    root.symlink("usr/bin/tool.real", "etc/alternatives/tool");
    let root_arg = root.path().to_string_lossy().to_string();

    let services = run(&["-nobanner", "-a", "s", "--root", &root_arg, "-c"]);
    assert!(services.contains("higher_module"), "services: {services}");
    assert!(services.contains("kept_module"), "services: {services}");
    assert!(!services.contains("lower_module"), "services: {services}");
    assert!(!services.contains("masked_module"), "services: {services}");
    assert!(!services.contains("ignored_module"), "services: {services}");

    let loader = run(&["-nobanner", "-a", "k", "--root", &root_arg, "-c"]);
    assert!(loader.contains("/opt/lib/first.so"), "loader: {loader}");
    assert!(loader.contains("/opt/lib/second.so"), "loader: {loader}");
    assert!(
        !loader.contains("first.so /opt/lib/second.so"),
        "loader: {loader}"
    );

    let hijacks = run(&["-nobanner", "-a", "h", "--root", &root_arg, "-c"]);
    let alternative = hijacks
        .lines()
        .find(|line| line.contains(",tool,"))
        .expect("alternative entry");
    assert!(
        alternative.contains("/usr/bin/tool.real"),
        "entry: {alternative}"
    );
}

#[test]
fn boot_and_network_hooks_enforce_effective_eligibility() {
    let root = TempRoot::new();
    root.write("etc/rc.local", "#!/bin/sh\nexit 0\n");
    root.write("etc/init.d/rc.local", "#!/bin/sh\nexit 0\n");
    root.write("etc/init.d/enabled-service", "#!/bin/sh\nexit 0\n");
    root.set_mode("etc/init.d/enabled-service", 0o755);
    root.symlink("etc/init.d/enabled-service", "etc/rc2.d/S01enabled-service");
    root.write(
        "etc/NetworkManager/dispatcher.d/not-executable",
        "#!/bin/sh\nexit 0\n",
    );
    root.write(
        "etc/NetworkManager/dispatcher.d/ignored.disabled",
        "#!/bin/sh\nexit 0\n",
    );
    root.set_mode("etc/NetworkManager/dispatcher.d/ignored.disabled", 0o755);
    root.write(
        "etc/NetworkManager/dispatcher.d/pre-up.d/nested-hook",
        "#!/bin/sh\nexit 0\n",
    );
    root.set_mode(
        "etc/NetworkManager/dispatcher.d/pre-up.d/nested-hook",
        0o755,
    );
    let root_arg = root.path().to_string_lossy().to_string();

    let boot = run(&["-nobanner", "-a", "b", "--root", &root_arg, "-c"]);
    let rc_rows: Vec<_> = boot
        .lines()
        .filter(|line| line.contains(",rc.local,"))
        .collect();
    assert_eq!(rc_rows.len(), 1, "boot: {boot}");
    assert!(rc_rows[0].contains("disabled"), "rc.local: {}", rc_rows[0]);
    let sysv = boot
        .lines()
        .find(|line| line.contains(",enabled-service,"))
        .expect("SysV entry");
    assert!(sysv.contains("enabled"), "SysV entry: {sysv}");

    let network = run(&["-nobanner", "-a", "n", "--root", &root_arg, "-c"]);
    assert!(network.contains("nested-hook"), "network: {network}");
    assert!(!network.contains("not-executable"), "network: {network}");
    assert!(!network.contains("ignored.disabled"), "network: {network}");
}

#[test]
fn json_output_is_wellformed_array() {
    let root = TempRoot::new();
    populate(&root);
    let root_arg = root.path().to_string_lossy().to_string();

    let stdout = run(&["-nobanner", "-a", "l", "--root", &root_arg, "--json"]);

    let parsed: serde_json::Value = serde_json::from_str(&stdout).expect("valid JSON output");
    let entries = parsed.as_array().expect("top-level JSON array");
    assert!(entries.iter().any(|entry| {
        entry.get("category").and_then(serde_json::Value::as_str) == Some("Logon")
            && entry.get("name").and_then(serde_json::Value::as_str) == Some("Example Autostart")
    }));
}

#[test]
fn tsv_and_xml_outputs_preserve_the_shared_schema() {
    let root = TempRoot::new();
    populate(&root);
    let root_arg = root.path().to_string_lossy().to_string();

    let tsv = run(&["-nobanner", "-a", "l", "--root", &root_arg, "-ct"]);
    let header = tsv.lines().next().expect("TSV header");
    assert_eq!(header.split('\t').count(), 22, "header: {header}");
    assert!(header.ends_with("Target\tCompleteness\tTargetState\tTargetExists\tTargetExecutable"));

    let xml = run(&["-nobanner", "-a", "l", "--root", &root_arg, "-x"]);
    let mut reader = quick_xml::Reader::from_str(&xml);
    let mut elements = HashSet::new();
    loop {
        match reader.read_event().expect("valid XML output") {
            quick_xml::events::Event::Start(element) => {
                elements.insert(String::from_utf8_lossy(element.local_name().as_ref()).to_string());
            }
            quick_xml::events::Event::Eof => break,
            _ => {}
        }
    }
    for expected in [
        "autoruns",
        "entry",
        "category",
        "status",
        "name",
        "imagePath",
        "source",
        "event",
        "mechanism",
        "principal",
        "profile",
        "activator",
        "target",
        "completeness",
        "targetState",
        "targetExists",
        "targetExecutable",
    ] {
        assert!(elements.contains(expected), "missing <{expected}>: {xml}");
    }
}

#[test]
fn csv_output_has_header() {
    let root = TempRoot::new();
    populate(&root);
    let root_arg = root.path().to_string_lossy().to_string();

    let stdout = run(&["-nobanner", "-a", "l", "--root", &root_arg, "-c"]);

    let header = stdout.lines().next().expect("csv header");
    assert_eq!(
        header,
        "Category,Status,Name,Description,Publisher,ImagePath,Command,Location,Source,Timestamp,SHA256,Note,Event,Mechanism,Principal,Profile,Activator,Target,Completeness,TargetState,TargetExists,TargetExecutable"
    );
}

#[test]
fn invalid_roots_fail_instead_of_reporting_an_empty_scan() {
    let root = TempRoot::new();
    let missing = root.path().join("missing");
    let missing_arg = missing.to_string_lossy().to_string();
    let output = run_output(&["-nobanner", "--root", &missing_arg, "-c"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("invalid --root"));

    root.write("not-a-directory", "content");
    let file_arg = root.path().join("not-a-directory");
    let file_arg = file_arg.to_string_lossy().to_string();
    let output = run_output(&["-nobanner", "--root", &file_arg, "-c"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("not a directory"));
}

#[test]
fn unreadable_sources_produce_partial_scan_status() {
    let root = TempRoot::new();
    root.write(
        "etc/xdg/autostart/unreadable.desktop",
        "[Desktop Entry]\nName=Unreadable\nExec=/bin/true\n",
    );
    root.set_mode("etc/xdg/autostart/unreadable.desktop", 0o000);
    let root_arg = root.path().to_string_lossy().to_string();

    let output = run_output(&["-nobanner", "-a", "l", "--root", &root_arg, "-c"]);

    root.set_mode("etc/xdg/autostart/unreadable.desktop", 0o600);
    assert_eq!(output.status.code(), Some(3));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("partial scan"), "stderr: {stderr}");
    assert!(stderr.contains("unreadable.desktop"), "stderr: {stderr}");
}

#[test]
fn hashes_in_process_and_ignores_path_helpers() {
    let root = TempRoot::new();
    root.write("etc/ld.so.preload", "/opt/payload\n");
    root.write("opt/payload", "abc");
    root.write(
        "fake-bin/sha256sum",
        "#!/bin/sh\ntouch \"$AUTORUNS_HASH_PROOF\"\nprintf not-a-hash\n",
    );
    root.set_mode("fake-bin/sha256sum", 0o755);
    let root_arg = root.path().to_string_lossy().to_string();
    let fake_path = root.path().join("fake-bin");
    let proof = root.path().join("helper-ran");

    let output = Command::new(BIN)
        .args(["-nobanner", "-a", "k", "--root", &root_arg, "-h"])
        .env("PATH", fake_path)
        .env("AUTORUNS_HASH_PROOF", &proof)
        .output()
        .expect("run autoruns binary");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("utf-8 stdout");
    assert!(
        stdout.contains("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"),
        "stdout: {stdout}"
    );
    assert!(
        !proof.exists(),
        "PATH-controlled sha256sum must not execute"
    );
}

#[test]
fn hash_failures_are_path_specific_partial_scan_diagnostics() {
    let root = TempRoot::new();
    root.write(
        "etc/ld.so.preload",
        "/opt/unreadable-one /opt/unreadable-two /opt/unreadable-one\n",
    );
    root.write("opt/unreadable-one", "one");
    root.write("opt/unreadable-two", "two");
    root.set_mode("opt/unreadable-one", 0o000);
    root.set_mode("opt/unreadable-two", 0o000);
    let root_arg = root.path().to_string_lossy().to_string();

    let output = run_output(&["-nobanner", "-a", "k", "--root", &root_arg, "-h", "--json"]);

    root.set_mode("opt/unreadable-one", 0o600);
    root.set_mode("opt/unreadable-two", 0o600);
    assert_eq!(output.status.code(), Some(3));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(
        stderr.matches("hash /opt/unreadable-one").count(),
        1,
        "stderr: {stderr}"
    );
    assert_eq!(
        stderr.matches("hash /opt/unreadable-two").count(),
        1,
        "stderr: {stderr}"
    );

    let stdout = String::from_utf8(output.stdout).expect("utf-8 stdout");
    let parsed: serde_json::Value = serde_json::from_str(&stdout).expect("valid JSON output");
    for path in ["/opt/unreadable-one", "/opt/unreadable-two"] {
        let rows: Vec<_> = parsed
            .as_array()
            .expect("top-level JSON array")
            .iter()
            .filter(|entry| {
                entry.get("imagePath").and_then(serde_json::Value::as_str) == Some(path)
            })
            .collect();
        assert!(!rows.is_empty(), "missing {path}: {stdout}");
        assert!(
            rows.iter().all(|entry| entry["sha256"] == ""),
            "unexpected hash for {path}: {rows:?}"
        );
    }
}

#[test]
fn table_escapes_terminal_controls_and_shows_requested_fields() {
    let root = TempRoot::new();
    root.write(
        "etc/xdg/autostart/control.desktop",
        "[Desktop Entry]\nName=Safe\u{001b}]52;c;dGVzdA==\u{0007}Name\nExec=/bin/true\n",
    );
    let root_arg = root.path().to_string_lossy().to_string();

    let output = run_output(&["-nobanner", "-a", "l", "--root", &root_arg, "-h", "-t"]);

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!output.stdout.contains(&0x1b));
    let stdout = String::from_utf8(output.stdout).expect("utf-8 stdout");
    assert!(stdout.contains("Timestamp"), "stdout: {stdout}");
    assert!(stdout.contains("SHA256"), "stdout: {stdout}");
    assert!(stdout.contains("\\u{001b}"), "stdout: {stdout}");
}

#[test]
fn output_files_are_owner_only() {
    let root = TempRoot::new();
    let root_arg = root.path().to_string_lossy().to_string();
    let report = root.path().join("report.csv");
    let report_arg = report.to_string_lossy().to_string();

    let output = run_output(&["-nobanner", "--root", &root_arg, "-c", "-o", &report_arg]);

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        fs::metadata(report)
            .expect("report metadata")
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
}

#[test]
fn symlinked_output_destination_is_rejected_without_modifying_target() {
    let root = TempRoot::new();
    let root_arg = root.path().to_string_lossy().to_string();
    root.write("victim", "preserve me");
    root.set_mode("victim", 0o640);
    unix_fs::symlink(root.path().join("victim"), root.path().join("report.csv"))
        .expect("create output symlink");
    let report_arg = root.path().join("report.csv").to_string_lossy().to_string();

    let output = run_output(&["-nobanner", "--root", &root_arg, "-c", "-o", &report_arg]);

    assert_eq!(output.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("failed to write"),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        fs::read_to_string(root.path().join("victim")).unwrap(),
        "preserve me"
    );
    assert_eq!(
        fs::metadata(root.path().join("victim"))
            .expect("victim metadata")
            .permissions()
            .mode()
            & 0o777,
        0o640
    );
}

#[test]
fn unsupported_security_flags_fail_closed() {
    for flag in ["-m", "-s", "-u", "-v"] {
        let output = run_output(&["-nobanner", flag]);
        assert_eq!(output.status.code(), Some(2), "flag {flag}");
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("unsupported security option"),
            "flag {flag}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let output = run_output(&["-nobanner", "-verbose"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("unknown option"));
}

#[test]
fn unknown_option_fails_with_usage() {
    let output = Command::new(BIN)
        .arg("--not-a-flag")
        .output()
        .expect("run autoruns binary");
    assert!(!output.status.success(), "unknown flag should fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("unknown option"), "stderr: {stderr}");
    assert!(
        stderr.contains("Usage:"),
        "usage text should be printed on failure: {stderr}"
    );
}
