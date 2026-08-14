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
            .label()
            .cmp(right.category.label())
            .then_with(|| left.location.cmp(&right.location))
            .then_with(|| left.name.cmp(&right.name))
    });
    entries
}

pub(crate) fn rooted(options: &Options, relative: &str) -> std::path::PathBuf {
    let relative = relative.trim_start_matches('/');
    options.root.join(relative)
}

pub(crate) fn read_to_string(root: &std::path::Path, path: &std::path::Path) -> Option<String> {
    std::fs::read_to_string(resolve_in_root(root, path)).ok()
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
    shell_tokens(command)
        .into_iter()
        .find(|token| !is_env_assignment(token))
        .map(|token| strip_exec_prefixes(&token).to_string())
        .filter(|token| !token.is_empty())
        .map(std::path::PathBuf::from)
}

// systemd `ExecStart=` executables may carry one or more special prefixes
// ("@", "-", ":", "+", "!", and "!!") that control how the command is run.
// They are not part of the executable path, so strip any leading run of them
// before treating the token as a path.
fn strip_exec_prefixes(token: &str) -> &str {
    token.trim_start_matches(['@', '-', ':', '+', '!'])
}

fn is_env_assignment(token: &str) -> bool {
    match token.find('=') {
        Some(position) if position > 0 => {
            let key = &token[..position];
            key.starts_with(|first: char| first.is_ascii_alphabetic() || first == '_')
                && key
                    .chars()
                    .all(|value| value.is_ascii_alphanumeric() || value == '_')
        }
        _ => false,
    }
}

fn shell_tokens(command: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut token = String::new();
    let mut has_token = false;
    let mut chars = command.trim().chars();
    let mut quote = None;

    while let Some(character) = chars.next() {
        match (quote, character) {
            (None, '#') if !has_token => break,
            (None, '\'') | (None, '"') => {
                quote = Some(character);
                has_token = true;
            }
            (Some(current), value) if value == current => quote = None,
            (None, ';') => {
                // An unquoted ';' is a shell command separator. Terminating the
                // token here keeps the leading command clean when several are
                // joined together (e.g. concatenated systemd ExecStart values).
                if has_token {
                    tokens.push(std::mem::take(&mut token));
                    has_token = false;
                }
            }
            (None, value) if value.is_whitespace() => {
                if has_token {
                    tokens.push(std::mem::take(&mut token));
                    has_token = false;
                }
            }
            (state, '\\') if state != Some('\'') => {
                if let Some(next) = chars.next() {
                    token.push(next);
                    has_token = true;
                }
            }
            (_, value) => {
                token.push(value);
                has_token = true;
            }
        }
    }

    if has_token {
        tokens.push(token);
    }
    tokens
}

pub(crate) fn modified_timestamp(root: &std::path::Path, path: &std::path::Path) -> Option<String> {
    let modified = std::fs::metadata(resolve_in_root(root, path))
        .ok()?
        .modified()
        .ok()?;
    let duration = modified.duration_since(std::time::UNIX_EPOCH).ok()?;
    Some(duration.as_secs().to_string())
}

// Reports the entry's location as an absolute path inside the scanned image
// (leading `/`), so it stays consistent with image_path/command rather than
// leaking the host mount point of a non-default --root.
pub(crate) fn display_location(path: &std::path::Path, root: &std::path::Path) -> String {
    in_root_path(path, root).display().to_string()
}

// Converts a rooted host path (under --root) back to its absolute path inside
// the scanned filesystem, so image_path/command stay independent of where the
// root is mounted. A no-op when scanning the live root.
pub(crate) fn in_root_path(path: &std::path::Path, root: &std::path::Path) -> std::path::PathBuf {
    match path.strip_prefix(root) {
        Ok(relative) => std::path::Path::new("/").join(relative),
        Err(_) => path.to_path_buf(),
    }
}

// Maximum symlink hops to follow while re-anchoring under --root. Mirrors the
// usual kernel limit and guards against cyclic links.
const MAX_SYMLINK_HOPS: usize = 40;

// Resolves `path` for reading or stat-ing so a symlink can never escape a
// non-default `--root`. Scanning the live root ("/") is a passthrough: normal
// symlink following is correct there because the host *is* the scanned image
// (enabled systemd units, /bin/sh, and many /etc entries are symlinks that must
// be followed). Under an offline image root, each symlink is resolved by hand
// and any absolute target is re-anchored under `root`, so a link such as
// /etc/systemd/system/default.target -> /lib/systemd/system/graphical.target
// reads the in-image file rather than the host's.
pub(crate) fn resolve_in_root(
    root: &std::path::Path,
    path: &std::path::Path,
) -> std::path::PathBuf {
    if root == std::path::Path::new("/") {
        return path.to_path_buf();
    }
    let mut current = path.to_path_buf();
    for _ in 0..MAX_SYMLINK_HOPS {
        let target = match std::fs::read_link(&current) {
            Ok(target) => target,
            Err(_) => break, // not a symlink (or unreadable): stop here
        };
        current = if target.is_absolute() {
            anchor_under_root(root, &target)
        } else if let Some(parent) = current.parent() {
            parent.join(target)
        } else {
            anchor_under_root(root, &target)
        };
    }
    current
}

// Re-anchors an absolute path under `root` (e.g. /lib/x under /mnt/img becomes
// /mnt/img/lib/x). A no-op for relative paths.
fn anchor_under_root(root: &std::path::Path, absolute: &std::path::Path) -> std::path::PathBuf {
    match absolute.strip_prefix("/") {
        Ok(relative) => root.join(relative),
        Err(_) => absolute.to_path_buf(),
    }
}
