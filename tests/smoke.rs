//! CLI smoke tests that build a temporary filesystem root with representative
//! autorun fixtures and assert that each scanner category and output format
//! behaves as expected when driven through the `--root` option.

use std::fs;
use std::os::unix::fs as unix_fs;
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
        let unique = format!(
            "autoruns-smoke-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        );
        let path = std::env::temp_dir().join(unique);
        fs::create_dir_all(&path).expect("create temp root");
        Self { path }
    }

    fn write(&self, relative: &str, contents: &str) {
        let full = self.path.join(relative);
        if let Some(parent) = full.parent() {
            fs::create_dir_all(parent).expect("create fixture parent");
        }
        fs::write(&full, contents).expect("write fixture");
    }

    fn symlink(&self, relative_target: &str, relative_link: &str) {
        let link = self.path.join(relative_link);
        if let Some(parent) = link.parent() {
            fs::create_dir_all(parent).expect("create symlink parent");
        }
        unix_fs::symlink(self.path.join(relative_target), link).expect("create symlink");
    }

    fn path(&self) -> &Path {
        &self.path
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

    // Boot hooks.
    root.write("etc/rc.local", "#!/bin/sh\n/usr/local/bin/startup.sh\n");

    // Loader / known-DLL equivalents.
    root.write("etc/ld.so.preload", "/opt/lib/inject.so\n");

    // Network hooks.
    root.write(
        "etc/NetworkManager/dispatcher.d/50-hook",
        "#!/bin/sh\n/usr/bin/nethook\n",
    );
}

fn run(args: &[&str]) -> String {
    let output = Command::new(BIN)
        .args(args)
        .output()
        .expect("run autoruns binary");
    assert!(
        output.status.success(),
        "binary exited with failure: {:?}\nstderr: {}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("utf-8 stdout")
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
fn json_output_is_wellformed_array() {
    let root = TempRoot::new();
    populate(&root);
    let root_arg = root.path().to_string_lossy().to_string();

    let stdout = run(&["-nobanner", "-a", "l", "--root", &root_arg, "--json"]);

    let trimmed = stdout.trim();
    assert!(
        trimmed.starts_with('['),
        "json should start with [: {trimmed}"
    );
    assert!(trimmed.ends_with(']'), "json should end with ]: {trimmed}");
    assert!(trimmed.contains("\"category\": \"Logon\""));
    assert!(trimmed.contains("\"name\": \"Example Autostart\""));
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
        "Category,Status,Name,Description,Publisher,ImagePath,Command,Location,Source,Timestamp,SHA256,Note"
    );
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
