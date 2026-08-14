mod cli;
mod model;
mod output;
mod scanners;

use std::{fs, process::ExitCode};

use cli::{parse_args, OutputFormat};
use model::AutorunEntry;

fn main() -> ExitCode {
    let options = match parse_args(std::env::args().collect()) {
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

    if !options.no_banner {
        eprintln!("Autoruns for Linux v{}", env!("CARGO_PKG_VERSION"));
        eprintln!("Sysinternals - Linux autorun entry scanner");
        eprintln!();
    }

    let mut entries = scanners::scan(&options);

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

    if options.hide_microsoft
        || options.verify_signatures
        || options.show_unsigned_only
        || options.virus_total_check
    {
        entries.push(AutorunEntry::unsupported(
            model::Category::Unsupported,
            "Windows-compatible trust filtering",
            "Linux publisher/signature and VirusTotal parity is not implemented yet",
        ));
    }

    let rendered = match options.format {
        OutputFormat::Table => output::table(&entries),
        OutputFormat::Csv => output::delimited(&entries, ',', &options.root),
        OutputFormat::Tsv => output::delimited(&entries, '\t', &options.root),
        OutputFormat::Json => output::json(&entries, &options.root),
        OutputFormat::Xml => output::xml(&entries, &options.root),
    };

    if let Some(path) = &options.output_file {
        if let Err(error) = fs::write(path, rendered) {
            eprintln!("failed to write {}: {error}", path.display());
            return ExitCode::from(1);
        }
    } else {
        print!("{rendered}");
    }

    ExitCode::SUCCESS
}

fn add_hashes(options: &cli::Options, entries: &mut [AutorunEntry]) {
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
            if candidate.is_file() {
                entry.sha256 = sha256_file(&candidate).ok();
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

fn sha256_file(path: &std::path::Path) -> std::io::Result<String> {
    use std::process::Command;

    let output = Command::new("sha256sum").arg(path).output();
    if let Ok(output) = output {
        if output.status.success() {
            let text = String::from_utf8_lossy(&output.stdout);
            if let Some(hash) = text.split_whitespace().next() {
                return Ok(hash.to_string());
            }
        }
    }

    Err(std::io::Error::new(
        std::io::ErrorKind::NotFound,
        "sha256sum command is unavailable",
    ))
}
