mod cli;
mod model;
mod output;
mod scanners;

use std::io::{Read, Write};
use std::process::ExitCode;

use cli::parse_args;
use model::AutorunEntry;

fn main() -> ExitCode {
    let mut options = match parse_args(std::env::args().collect()) {
        Ok(options) => options,
        Err(message) => {
            eprintln!("{message}");
            eprintln!();
            eprintln!("{}", cli::usage());
            return ExitCode::from(2);
        }
    };

    if options.show_help {
        println!("{}", cli::usage());
        return ExitCode::SUCCESS;
    }

    options.root = match validate_root(&options.root) {
        Ok(root) => root,
        Err(message) => {
            eprintln!("{message}");
            return ExitCode::from(2);
        }
    };

    if !options.no_banner {
        eprintln!("Autoruns for Linux v{}", env!("CARGO_PKG_VERSION"));
        eprintln!("Sysinternals - Linux autorun entry scanner");
        eprintln!();
    }

    let report = scanners::scan(&options);
    let mut entries = report.entries;

    if options.show_hashes {
        add_hashes(&options, &mut entries);
    }

    if options.utc_timestamps {
        for entry in &mut entries {
            if let Some(timestamp) = entry.timestamp.take() {
                entry.timestamp = Some(format_utc_timestamp(&timestamp));
            }
        }
    }

    for diagnostic in &report.diagnostics {
        eprintln!(
            "autoruns: partial scan: {} {}: {}",
            diagnostic.operation,
            diagnostic.path.display(),
            diagnostic.message
        );
    }

    if let Some(path) = &options.output_file {
        let result = create_private_file(path).and_then(|file| {
            let mut writer = std::io::BufWriter::new(file);
            output::write(&mut writer, &entries, &options.format, &options.root)?;
            writer.flush()
        });
        if let Err(error) = result {
            eprintln!("failed to write {}: {error}", path.display());
            return ExitCode::from(1);
        }
    } else {
        let stdout = std::io::stdout();
        let mut writer = std::io::BufWriter::new(stdout.lock());
        let result = output::write(&mut writer, &entries, &options.format, &options.root)
            .and_then(|()| writer.flush());
        if let Err(error) = result {
            if error.kind() != std::io::ErrorKind::BrokenPipe {
                eprintln!("failed to write output: {error}");
                return ExitCode::from(1);
            }
        }
    }

    if report.diagnostics.is_empty() {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(3)
    }
}

fn validate_root(root: &std::path::Path) -> Result<std::path::PathBuf, String> {
    let root = std::fs::canonicalize(root)
        .map_err(|error| format!("invalid --root {}: {error}", root.display()))?;
    if !root.is_dir() {
        return Err(format!(
            "invalid --root {}: not a directory",
            root.display()
        ));
    }
    Ok(root)
}

#[cfg(unix)]
fn create_private_file(path: &std::path::Path) -> std::io::Result<std::fs::File> {
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

    let file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)?;
    file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    Ok(file)
}

#[cfg(not(unix))]
fn create_private_file(path: &std::path::Path) -> std::io::Result<std::fs::File> {
    std::fs::File::create(path)
}

fn add_hashes(options: &cli::Options, entries: &mut [AutorunEntry]) {
    let mut warned = false;
    for entry in entries {
        if let Some(path) = entry.image_path.as_ref() {
            // Only hash absolute in-image paths. A relative image path (e.g. a
            // unit or cron entry that runs `bash` rather than `/bin/bash`) must
            // not be resolved against the current working directory, which could
            // hash an unrelated host file outside the scanned --root.
            if !path.is_absolute() {
                continue;
            }
            let candidate = resolve_under_root(&options.root, path);
            if scanners::path_is_file(&options.root, &candidate) {
                match sha256_file(&options.root, &candidate) {
                    Ok(hash) => entry.sha256 = Some(hash),
                    Err(error) => {
                        if !warned {
                            eprintln!("autoruns: unable to compute file hashes: {error}");
                            warned = true;
                        }
                    }
                }
            }
        }
    }
}

// Renders an epoch-seconds timestamp as an ISO-8601 UTC string for `-t`.
// Non-numeric values are returned unchanged.
fn format_utc_timestamp(epoch: &str) -> String {
    let seconds: i64 = match epoch.parse() {
        Ok(value) => value,
        Err(_) => return epoch.to_string(),
    };
    let days = seconds.div_euclid(86_400);
    let time_of_day = seconds.rem_euclid(86_400);
    let (hour, minute, second) = (
        time_of_day / 3_600,
        (time_of_day % 3_600) / 60,
        time_of_day % 60,
    );
    let (year, month, day) = civil_from_days(days);
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

// Howard Hinnant's days-to-civil-date algorithm (proleptic Gregorian, UTC),
// mapping a day count relative to the Unix epoch to (year, month, day).
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let day_of_era = z - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_portion = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_portion + 2) / 5 + 1;
    let month = if month_portion < 10 {
        month_portion + 3
    } else {
        month_portion - 9
    };
    let year = if month <= 2 { year + 1 } else { year };
    (year, month, day)
}

// image_path values are absolute paths inside the scanned filesystem. When a
// non-default --root is used they must be re-anchored under that root before the
// on-disk file can be hashed; paths already under the root are left untouched.
fn resolve_under_root(root: &std::path::Path, path: &std::path::Path) -> std::path::PathBuf {
    if root == std::path::Path::new("/") || path.starts_with(root) {
        return path.to_path_buf();
    }
    match path.strip_prefix("/") {
        Ok(relative) => root.join(relative),
        Err(_) => path.to_path_buf(),
    }
}

fn sha256_file(root: &std::path::Path, path: &std::path::Path) -> std::io::Result<String> {
    use sha2::{Digest, Sha256};

    let mut file = scanners::open_file_in_root(root, path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}
