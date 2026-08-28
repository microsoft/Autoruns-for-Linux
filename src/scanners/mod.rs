mod applications;
mod browser;
mod cron;
mod desktop;
mod device;
mod linux;
mod shell;
mod systemd_effective;

use crate::{
    cli::Options,
    model::{AutorunEntry, Category, ScanDiagnostic, TargetState},
};

#[derive(Debug, Default)]
pub struct ScanReport {
    pub entries: Vec<AutorunEntry>,
    pub diagnostics: Vec<ScanDiagnostic>,
}

thread_local! {
    static DIAGNOSTICS: std::cell::RefCell<Vec<ScanDiagnostic>> = const { std::cell::RefCell::new(Vec::new()) };
    static ROOT_ACCESS: std::cell::RefCell<Option<RootAccess>> = const { std::cell::RefCell::new(None) };
}

#[derive(Clone, Copy)]
enum RootAccessMode {
    Openat2,
    ImmutableFallback,
}

struct RootAccess {
    path: std::path::PathBuf,
    directory: std::fs::File,
    mode: RootAccessMode,
    fallback_reason: Option<String>,
}

pub fn scan(options: &Options) -> ScanReport {
    DIAGNOSTICS.with(|diagnostics| diagnostics.borrow_mut().clear());
    match initialize_root_access(&options.root) {
        Ok(Some(reason)) => record_diagnostic(
            "containment",
            &options.root,
            format!(
                "descriptor-relative openat2 containment is unavailable ({reason}); the alternate root must remain immutable during the scan"
            ),
        ),
        Ok(None) => {}
        Err(error) => record_diagnostic("open scan root", &options.root, error),
    }
    let mut entries = Vec::new();
    let include_services = options.categories.contains(&Category::Services);
    let include_timers = options.categories.contains(&Category::ScheduledTasks);
    let include_devices = options.categories.contains(&Category::DeviceMount);

    for category in &options.categories {
        match category {
            Category::Logon => {
                entries.extend(desktop::scan(options));
                entries.extend(shell::scan(options));
            }
            Category::Services => {
                entries.extend(linux::scan_modules(options));
            }
            Category::ScheduledTasks => {
                entries.extend(cron::scan(options));
            }
            Category::Boot => entries.extend(linux::scan_boot(options)),
            Category::Hijacks => entries.extend(linux::scan_hijacks(options)),
            Category::Loader => entries.extend(linux::scan_loader(options)),
            Category::Network => entries.extend(linux::scan_network(options)),
            Category::Browser => entries.extend(browser::scan(options)),
            Category::DeviceMount => entries.extend(device::scan(options)),
            Category::ApplicationIntegrations => entries.extend(applications::scan(options)),
            Category::Unsupported => entries.push(AutorunEntry::unsupported(
                Category::Unsupported,
                "Windows-only Autoruns category",
                "This selector does not currently have a Linux scanner",
            )),
        }
    }

    entries.extend(systemd_effective::scan(
        options,
        include_services,
        include_timers,
        include_devices,
    ));
    entries.extend(completeness_limits(options));
    enrich_target_metadata(options, &mut entries);

    entries.sort_by(|left, right| {
        left.category
            .label()
            .cmp(right.category.label())
            .then_with(|| left.location.cmp(&right.location))
            .then_with(|| left.name.cmp(&right.name))
    });
    let diagnostics =
        DIAGNOSTICS.with(|diagnostics| std::mem::take(&mut *diagnostics.borrow_mut()));
    ScanReport {
        entries,
        diagnostics,
    }
}

fn enrich_target_metadata(options: &Options, entries: &mut [AutorunEntry]) {
    use std::os::unix::fs::PermissionsExt;

    for entry in entries {
        let Some(path) = entry.image_path.as_ref() else {
            continue;
        };
        if !path.is_absolute() {
            entry.target_state = Some(TargetState::Unresolved);
            continue;
        }

        match metadata_in_root(&options.root, path) {
            Ok(metadata) => {
                entry.target_state = Some(TargetState::Present);
                entry.target_exists = Some(true);
                entry.target_executable =
                    Some(metadata.is_file() && metadata.permissions().mode() & 0o111 != 0);
            }
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory
                ) =>
            {
                entry.target_state = Some(TargetState::Missing);
                entry.target_exists = Some(false);
                entry.target_executable = Some(false);
            }
            Err(error) => {
                entry.target_state = Some(TargetState::Inaccessible);
                record_diagnostic("inspect target", path, error);
            }
        }
    }
}

fn completeness_limits(options: &Options) -> Vec<AutorunEntry> {
    let mut entries = Vec::new();
    let limits = [
        (
            Category::Logon,
            "Unsupported logon adapters",
            "user logon or desktop session",
            "PAM, display-manager, compositor, and desktop-specific startup",
            "XDG autostart and supported shell profiles are inspected; host-specific session adapters are not",
        ),
        (
            Category::Services,
            "Runtime-only service state",
            "service manager activation",
            "transient/generated runtime units and live D-Bus state",
            "static systemd load paths are inspected; transient units and runtime-only manager state require live inspection",
        ),
        (
            Category::ScheduledTasks,
            "Unsupported scheduler adapters",
            "scheduled execution",
            "at/batch queues and application-specific schedulers",
            "cron, anacron, and systemd timers are inspected; other scheduler queues are outside the supported adapter set",
        ),
        (
            Category::Boot,
            "Unsupported early-boot adapters",
            "early system boot",
            "bootloader, kernel command line, initramfs, and generator output",
            "rc.local and SysV hooks are inspected; bootloader and initramfs mechanisms are not",
        ),
        (
            Category::Hijacks,
            "Unsupported command-resolution adapters",
            "command or process launch",
            "environment-only and application-specific command interception",
            "alternatives registrations are inspected; ephemeral environment and host-specific interception are not",
        ),
        (
            Category::Loader,
            "Unsupported loader adapters",
            "process image loading",
            "runtime environment and namespace-specific loader injection",
            "ld.so preload/search configuration is inspected; process-local runtime environment is not",
        ),
        (
            Category::Network,
            "Unsupported network-daemon adapters",
            "network state transition",
            "daemon-specific hooks outside supported NetworkManager, ifupdown, and DHCP paths",
            "published hook directories are inspected; arbitrary network-daemon plugin systems are not",
        ),
        (
            Category::Browser,
            "Unsupported browser/profile adapters",
            "browser or profile startup",
            "unsupported browser products and runtime-only profile arguments",
            "published Chromium-family and Firefox layouts are inspected; arbitrary products and non-persistent runtime profiles are not",
        ),
        (
            Category::DeviceMount,
            "Unavailable device/media state",
            "device, mount, path, or media activation",
            "unmounted media and runtime-only udev state",
            "static rules/configuration and already mounted media are inspected; devices are never synthesized or mounted",
        ),
        (
            Category::ApplicationIntegrations,
            "Unsupported application hosts",
            "application-defined extension activation",
            "application plugin systems other than LibreOffice/OpenOffice",
            "LibreOffice/OpenOffice integrations are inspected; Linux has no universal application plugin registry",
        ),
    ];
    for (category, name, event, mechanism, note) in limits {
        if options.categories.contains(&category) {
            entries.push(AutorunEntry::completeness_limit(
                category, name, event, mechanism, note,
            ));
        }
    }
    entries
}

pub(crate) fn record_diagnostic(
    operation: impl Into<String>,
    path: &std::path::Path,
    error: impl std::fmt::Display,
) {
    DIAGNOSTICS.with(|diagnostics| {
        diagnostics.borrow_mut().push(ScanDiagnostic::new(
            operation,
            path.to_path_buf(),
            error.to_string(),
        ));
    });
}

pub(crate) fn rooted(options: &Options, relative: &str) -> std::path::PathBuf {
    let relative = relative.trim_start_matches('/');
    options.root.join(relative)
}

// Returns the user home directories to scan for per-user startup/autostart
// entries: every directory under /home, the root account's home (/root), and --
// only when scanning the live root -- the current $HOME. Paths are rooted under
// --root and deduplicated. Including /root explicitly means the root account's
// entries are covered when scanning an offline image (where $HOME is irrelevant)
// and when scanning the live system as a non-root user.
pub(crate) fn home_dirs(options: &Options) -> Vec<std::path::PathBuf> {
    user_homes(options)
        .into_iter()
        .map(|user| user.path)
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct UserHome {
    pub principal: String,
    pub path: std::path::PathBuf,
}

pub(crate) fn user_homes(options: &Options) -> Vec<UserHome> {
    let mut homes = Vec::new();
    let passwd = rooted(options, "/etc/passwd");
    if let Some(content) = read_to_string(&options.root, &passwd) {
        for line in content.lines() {
            let fields: Vec<&str> = line.split(':').collect();
            if fields.len() < 7 || !fields[5].starts_with('/') {
                continue;
            }
            let path = rooted(options, fields[5]);
            if path_is_dir(&options.root, &path) {
                homes.push(UserHome {
                    principal: fields[0].to_string(),
                    path,
                });
            }
        }
    }

    for path in list_dirs(&options.root, &rooted(options, "/home")) {
        let principal = path
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| "unknown".to_string());
        homes.push(UserHome { principal, path });
    }

    let root_home = rooted(options, "/root");
    if path_is_dir(&options.root, &root_home) {
        homes.push(UserHome {
            principal: "root".to_string(),
            path: root_home,
        });
    }

    if options.root == std::path::Path::new("/") {
        if let Ok(home) = std::env::var("HOME") {
            let principal = std::env::var("USER").unwrap_or_else(|_| "current user".to_string());
            homes.push(UserHome {
                principal,
                path: std::path::PathBuf::from(home),
            });
        }
    }

    homes.sort_by(|left, right| left.path.cmp(&right.path));
    homes.dedup_by(|left, right| left.path == right.path);
    homes
}

pub(crate) fn read_to_string(root: &std::path::Path, path: &std::path::Path) -> Option<String> {
    let mut file = match open_file_in_root(root, path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return None,
        Err(error) => {
            record_diagnostic("read", path, error);
            return None;
        }
    };
    let mut content = String::new();
    match std::io::Read::read_to_string(&mut file, &mut content) {
        Ok(_) => Some(content),
        Err(error) => {
            record_diagnostic("read", path, error);
            None
        }
    }
}

pub(crate) fn open_file_in_root(
    root: &std::path::Path,
    path: &std::path::Path,
) -> std::io::Result<std::fs::File> {
    if root == std::path::Path::new("/") {
        return std::fs::File::open(path);
    }

    open_within_root(
        root,
        path,
        rustix::fs::OFlags::RDONLY | rustix::fs::OFlags::CLOEXEC,
    )
}

pub(crate) fn list_files(root: &std::path::Path, dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut files = Vec::new();
    match directory_entries(root, dir) {
        Ok(entries) => {
            for (name, file_type) in entries {
                let path = dir.join(name);
                if matches!(
                    file_type,
                    rustix::fs::FileType::RegularFile | rustix::fs::FileType::Symlink
                ) || (file_type == rustix::fs::FileType::Unknown && path_is_file(root, &path))
                {
                    files.push(path);
                }
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => record_diagnostic("list", dir, error),
    }
    files.sort();
    files
}

pub(crate) fn list_dirs(root: &std::path::Path, dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut dirs = Vec::new();
    match directory_entries(root, dir) {
        Ok(entries) => {
            for (name, file_type) in entries {
                let path = dir.join(name);
                if file_type == rustix::fs::FileType::Directory
                    || matches!(
                        file_type,
                        rustix::fs::FileType::Symlink | rustix::fs::FileType::Unknown
                    ) && path_is_dir(root, &path)
                {
                    dirs.push(path);
                }
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => record_diagnostic("list", dir, error),
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

pub(crate) fn is_executable_file(root: &std::path::Path, path: &std::path::Path) -> bool {
    use std::os::unix::fs::PermissionsExt;

    metadata_in_root(root, path)
        .ok()
        .map(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
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

pub(crate) fn shell_tokens(command: &str) -> Vec<String> {
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
    let metadata = match metadata_in_root(root, path) {
        Ok(metadata) => metadata,
        Err(error) => {
            record_diagnostic("metadata", path, error);
            return None;
        }
    };
    let modified = match metadata.modified() {
        Ok(modified) => modified,
        Err(error) => {
            record_diagnostic("metadata", path, error);
            return None;
        }
    };
    let duration = modified.duration_since(std::time::UNIX_EPOCH).ok()?;
    Some(duration.as_secs().to_string())
}

pub(crate) fn metadata_in_root(
    root: &std::path::Path,
    path: &std::path::Path,
) -> std::io::Result<std::fs::Metadata> {
    if root == std::path::Path::new("/") {
        return std::fs::metadata(path);
    }
    open_within_root(
        root,
        path,
        rustix::fs::OFlags::PATH | rustix::fs::OFlags::CLOEXEC,
    )?
    .metadata()
}

pub(crate) fn path_is_file(root: &std::path::Path, path: &std::path::Path) -> bool {
    metadata_in_root(root, path)
        .map(|metadata| metadata.is_file())
        .unwrap_or(false)
}

pub(crate) fn path_is_dir(root: &std::path::Path, path: &std::path::Path) -> bool {
    metadata_in_root(root, path)
        .map(|metadata| metadata.is_dir())
        .unwrap_or(false)
}

pub(crate) fn directory_identity(
    root: &std::path::Path,
    path: &std::path::Path,
) -> Option<(u64, u64)> {
    use std::os::unix::fs::MetadataExt;

    metadata_in_root(root, path)
        .ok()
        .filter(|metadata| metadata.is_dir())
        .map(|metadata| (metadata.dev(), metadata.ino()))
}

pub(crate) fn read_link_in_root(
    root: &std::path::Path,
    path: &std::path::Path,
) -> std::io::Result<std::path::PathBuf> {
    use std::os::unix::ffi::OsStringExt;

    if root == std::path::Path::new("/") {
        return std::fs::read_link(path);
    }
    let parent = path.parent().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "path has no parent")
    })?;
    let name = path
        .file_name()
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidInput, "path has no name"))?;
    let directory = open_within_root(
        root,
        parent,
        rustix::fs::OFlags::RDONLY | rustix::fs::OFlags::DIRECTORY | rustix::fs::OFlags::CLOEXEC,
    )?;
    rustix::fs::readlinkat(&directory, name, Vec::new())
        .map(|target| std::path::PathBuf::from(std::ffi::OsString::from_vec(target.into_bytes())))
        .map_err(std::io::Error::from)
}

fn initialize_root_access(root: &std::path::Path) -> std::io::Result<Option<String>> {
    if root == std::path::Path::new("/") {
        ROOT_ACCESS.with(|access| *access.borrow_mut() = None);
        return Ok(None);
    }
    ROOT_ACCESS.with(|slot| {
        let mut slot = slot.borrow_mut();
        if let Some(access) = slot.as_ref() {
            if access.path == root {
                return Ok(access.fallback_reason.clone());
            }
        }

        let directory = std::fs::File::open(root)?;
        let probe = rustix::fs::openat2(
            &directory,
            ".",
            rustix::fs::OFlags::PATH | rustix::fs::OFlags::DIRECTORY | rustix::fs::OFlags::CLOEXEC,
            rustix::fs::Mode::empty(),
            rustix::fs::ResolveFlags::IN_ROOT | rustix::fs::ResolveFlags::NO_MAGICLINKS,
        );
        let (mode, fallback_reason) = match probe {
            Ok(_) => (RootAccessMode::Openat2, None),
            Err(error) => (
                RootAccessMode::ImmutableFallback,
                Some(std::io::Error::from(error).to_string()),
            ),
        };
        *slot = Some(RootAccess {
            path: root.to_path_buf(),
            directory,
            mode,
            fallback_reason: fallback_reason.clone(),
        });
        Ok(fallback_reason)
    })
}

fn open_within_root(
    root: &std::path::Path,
    path: &std::path::Path,
    flags: rustix::fs::OFlags,
) -> std::io::Result<std::fs::File> {
    initialize_root_access(root)?;
    ROOT_ACCESS.with(|slot| {
        let slot = slot.borrow();
        let access = slot.as_ref().ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::NotFound, "scan root is not open")
        })?;
        match access.mode {
            RootAccessMode::Openat2 => {
                let in_image = in_root_path(path, root);
                let relative = in_image.strip_prefix("/").unwrap_or(&in_image);
                rustix::fs::openat2(
                    &access.directory,
                    relative,
                    flags,
                    rustix::fs::Mode::empty(),
                    rustix::fs::ResolveFlags::IN_ROOT | rustix::fs::ResolveFlags::NO_MAGICLINKS,
                )
                .map(std::fs::File::from)
                .map_err(std::io::Error::from)
            }
            RootAccessMode::ImmutableFallback => {
                let resolved = resolve_in_root(root, path).ok_or_else(|| {
                    std::io::Error::new(
                        std::io::ErrorKind::PermissionDenied,
                        "path could not be resolved inside scan root",
                    )
                })?;
                std::fs::File::open(resolved)
            }
        }
    })
}

fn directory_entries(
    root: &std::path::Path,
    path: &std::path::Path,
) -> std::io::Result<Vec<(std::ffi::OsString, rustix::fs::FileType)>> {
    use std::os::unix::ffi::OsStringExt;

    let directory = if root == std::path::Path::new("/") {
        std::fs::File::open(path)?
    } else {
        open_within_root(
            root,
            path,
            rustix::fs::OFlags::RDONLY
                | rustix::fs::OFlags::DIRECTORY
                | rustix::fs::OFlags::CLOEXEC,
        )?
    };
    let mut buffer = vec![std::mem::MaybeUninit::<u8>::uninit(); 64 * 1024];
    let mut iterator = rustix::fs::RawDir::new(&directory, &mut buffer);
    let mut entries = Vec::new();
    while let Some(result) = iterator.next() {
        let entry = result.map_err(std::io::Error::from)?;
        let name = entry.file_name().to_bytes();
        if name != b"." && name != b".." {
            entries.push((
                std::ffi::OsString::from_vec(name.to_vec()),
                entry.file_type(),
            ));
        }
    }
    Ok(entries)
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
