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
    resolve_in_root(root, path).and_then(|resolved| std::fs::read_to_string(resolved).ok())
}

pub(crate) fn list_files(root: &std::path::Path, dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut files = Vec::new();
    // Resolve `dir` first so a symlinked directory (e.g. a usrmerge /lib ->
    // /usr/lib) cannot make `read_dir` follow an absolute link out to the host
    // when scanning a non-default --root.
    if let Some(resolved) = resolve_in_root(root, dir) {
        if let Ok(read_dir) = std::fs::read_dir(resolved) {
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
    }
    files.sort();
    files
}

pub(crate) fn list_dirs(root: &std::path::Path, dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut dirs = Vec::new();
    if let Some(resolved) = resolve_in_root(root, dir) {
        if let Ok(read_dir) = std::fs::read_dir(resolved) {
            for entry in read_dir.flatten() {
                let path = entry.path();
                // Include symlinks whose target (resolved under --root) is a
                // directory, e.g. a relocated home or usrmerge-style layout, so
                // scanners that rely on `list_dirs` do not silently skip them.
                let is_dir = match entry.file_type() {
                    Ok(kind) if kind.is_dir() => true,
                    Ok(kind) if kind.is_symlink() => resolve_in_root(root, &path)
                        .map(|resolved| resolved.is_dir())
                        .unwrap_or(false),
                    _ => false,
                };
                if is_dir {
                    dirs.push(path);
                }
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
    let modified = std::fs::metadata(resolve_in_root(root, path)?)
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
// be followed). Under an offline image root, the path is resolved one component
// at a time from the image root down: every component (not just the final one)
// is checked with `read_link`, and any symlink it holds is expanded and clamped
// to the image root before resolution continues. This means an intermediate
// component such as a hostile `/etc -> /` or `/etc/systemd/system -> /lib/...`
// is re-anchored under `root` instead of being followed onto the host, and a
// relative link like ../../etc/shadow cannot climb out of the image.
//
// Fails closed (returns `None`) when the target cannot be determined safely: a
// symlink chain longer than `MAX_SYMLINK_HOPS` (or a cycle), or a `read_link`
// error other than "not a symlink"/"not found". Returning a partially resolved
// path in those cases would let the OS follow a residual symlink out of the
// image on the next filesystem call, so callers get nothing instead.
pub(crate) fn resolve_in_root(
    root: &std::path::Path,
    path: &std::path::Path,
) -> Option<std::path::PathBuf> {
    if root == std::path::Path::new("/") {
        return Some(path.to_path_buf());
    }
    // Queue of in-image path components still to resolve. Symlink expansions are
    // pushed back onto the front so their own components are resolved in turn.
    let mut pending: std::collections::VecDeque<std::ffi::OsString> =
        components_of(&in_root_path(path, root));
    // The symlink-free in-image prefix resolved so far (always absolute).
    let mut resolved = std::path::PathBuf::from("/");
    let mut hops = 0usize;
    while let Some(part) = pending.pop_front() {
        if part == std::ffi::OsStr::new(".") {
            continue;
        }
        if part == std::ffi::OsStr::new("..") {
            // Clamped: popping at the image root is a no-op, so `..` can never
            // climb above "/".
            resolved.pop();
            continue;
        }
        let candidate = resolved.join(&part);
        let host = anchor_under_root(root, &candidate);
        match std::fs::read_link(&host) {
            Ok(link) => {
                hops += 1;
                if hops > MAX_SYMLINK_HOPS {
                    // A chain this long is either malicious or cyclic: fail closed.
                    return None;
                }
                // An absolute link restarts resolution from the image root; a
                // relative link resolves against the already-resolved parent.
                if link.is_absolute() {
                    resolved = std::path::PathBuf::from("/");
                }
                for component in components_of(&link).into_iter().rev() {
                    pending.push_front(component);
                }
            }
            // EINVAL ("not a symlink") and ENOENT ("no such path") both mean this
            // component holds no link to follow: accept it and move on.
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::InvalidInput | std::io::ErrorKind::NotFound
                ) =>
            {
                resolved = candidate;
            }
            // Any other error (e.g. PermissionDenied) leaves the component
            // undetermined: fail closed rather than risk following it out.
            Err(_) => return None,
        }
    }
    Some(anchor_under_root(root, &resolved))
}

// Extracts the ordinary path components (Normal, plus `.`/`..` preserved as
// literals) so a resolver can process them one at a time. Root/prefix markers
// are dropped because resolution always works in absolute in-image space.
fn components_of(path: &std::path::Path) -> std::collections::VecDeque<std::ffi::OsString> {
    use std::path::Component;
    let mut parts = std::collections::VecDeque::new();
    for component in path.components() {
        match component {
            Component::Normal(value) => parts.push_back(value.to_os_string()),
            Component::CurDir => parts.push_back(std::ffi::OsString::from(".")),
            Component::ParentDir => parts.push_back(std::ffi::OsString::from("..")),
            Component::Prefix(_) | Component::RootDir => {}
        }
    }
    parts
}

// Re-anchors an absolute path under `root` (e.g. /lib/x under /mnt/img becomes
// /mnt/img/lib/x). A no-op for relative paths.
fn anchor_under_root(root: &std::path::Path, absolute: &std::path::Path) -> std::path::PathBuf {
    match absolute.strip_prefix("/") {
        Ok(relative) => root.join(relative),
        Err(_) => absolute.to_path_buf(),
    }
}
