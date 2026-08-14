mod cron;
mod desktop;
mod linux;
mod shell;
mod systemd;

use crate::{
    cli::Options,
    model::{AutorunEntry, Category},
};

pub fn scan(options: &Options) -> Vec<AutorunEntry> {
    let mut entries = Vec::new();

    for category in &options.categories {
        match category {
            Category::Logon => {
                entries.extend(desktop::scan(options));
                entries.extend(shell::scan(options));
            }
            Category::Services => {
                entries.extend(systemd::scan_services(options));
                entries.extend(linux::scan_modules(options));
            }
            Category::ScheduledTasks => {
                entries.extend(cron::scan(options));
                entries.extend(systemd::scan_timers(options));
            }
            Category::Boot => entries.extend(linux::scan_boot(options)),
            Category::Hijacks => entries.extend(linux::scan_hijacks(options)),
            Category::Loader => entries.extend(linux::scan_loader(options)),
            Category::Network => entries.extend(linux::scan_network(options)),
            Category::Unsupported => entries.push(AutorunEntry::unsupported(
                Category::Unsupported,
                "Windows-only Autoruns category",
                "This selector does not currently have a Linux scanner",
            )),
        }
    }

    entries.sort_by(|left, right| {
        left.category
            .to_string()
            .cmp(&right.category.to_string())
            .then_with(|| left.location.cmp(&right.location))
            .then_with(|| left.name.cmp(&right.name))
    });
    entries
}

pub(crate) fn rooted(options: &Options, relative: &str) -> std::path::PathBuf {
    let relative = relative.trim_start_matches('/');
    options.root.join(relative)
}

pub(crate) fn read_to_string(path: &std::path::Path) -> Option<String> {
    std::fs::read_to_string(path).ok()
}

pub(crate) fn list_files(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut files = Vec::new();
    if let Ok(read_dir) = std::fs::read_dir(dir) {
        for entry in read_dir.flatten() {
            if entry
                .file_type()
                .map(|kind| kind.is_file() || kind.is_symlink())
                .unwrap_or(false)
            {
                files.push(entry.path());
            }
        }
    }
    files.sort();
    files
}

pub(crate) fn list_dirs(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut dirs = Vec::new();
    if let Ok(read_dir) = std::fs::read_dir(dir) {
        for entry in read_dir.flatten() {
            if entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false) {
                dirs.push(entry.path());
            }
        }
    }
    dirs.sort();
    dirs
}

pub(crate) fn first_command_path(command: &str) -> Option<std::path::PathBuf> {
    let token = first_shell_token(command)?;
    if token.contains('=') && !token.contains('/') {
        return None;
    }
    Some(std::path::PathBuf::from(token))
}

fn first_shell_token(command: &str) -> Option<String> {
    let mut token = String::new();
    let mut chars = command.trim().chars().peekable();
    let mut quote = None;

    while let Some(character) = chars.next() {
        match (quote, character) {
            (None, '#') if token.is_empty() => return None,
            (None, '\'') | (None, '"') => quote = Some(character),
            (Some(current), value) if value == current => quote = None,
            (None, value) if value.is_whitespace() => break,
            (_, '\\') => {
                if let Some(next) = chars.next() {
                    token.push(next);
                }
            }
            (_, value) => token.push(value),
        }
    }

    if token.is_empty() {
        None
    } else {
        Some(token)
    }
}

pub(crate) fn modified_timestamp(path: &std::path::Path) -> Option<String> {
    let modified = std::fs::metadata(path).ok()?.modified().ok()?;
    let duration = modified.duration_since(std::time::UNIX_EPOCH).ok()?;
    Some(duration.as_secs().to_string())
}

pub(crate) fn display_location(path: &std::path::Path, root: &std::path::Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .display()
        .to_string()
}
