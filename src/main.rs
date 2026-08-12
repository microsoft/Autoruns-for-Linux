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
        add_hashes(&mut entries);
    }

    if options.hide_microsoft || options.verify_signatures || options.show_unsigned_only || options.virus_total_check {
        entries.push(AutorunEntry::unsupported(
            model::Category::Unsupported,
            "Windows-compatible trust filtering",
            "Linux publisher/signature and VirusTotal parity is not implemented yet",
        ));
    }

    let rendered = match options.format {
        OutputFormat::Table => output::table(&entries),
        OutputFormat::Csv => output::delimited(&entries, ','),
        OutputFormat::Tsv => output::delimited(&entries, '\t'),
        OutputFormat::Json => output::json(&entries),
        OutputFormat::Xml => output::xml(&entries),
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

fn add_hashes(entries: &mut [AutorunEntry]) {
    for entry in entries {
        if let Some(path) = entry.image_path.as_ref() {
            if path.is_file() {
                entry.sha256 = sha256_file(path).ok();
            }
        }
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