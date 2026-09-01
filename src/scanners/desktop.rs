use std::collections::{HashMap, HashSet};

use crate::{
    cli::Options,
    model::{AutorunEntry, Category, EntryStatus},
};

use super::{
    display_location, first_command_path, is_executable_file, list_files, modified_timestamp,
    read_to_string, rooted, user_homes,
};

pub fn scan(options: &Options) -> Vec<AutorunEntry> {
    let mut entries = Vec::new();
    let system_dirs = system_autostart_dirs(options);
    let users = user_homes(options);

    if users.is_empty() {
        scan_scope(options, "all users", None, &system_dirs, &mut entries);
    } else {
        for user in users {
            scan_scope(
                options,
                &user.principal,
                Some(user.path.join(".config/autostart")),
                &system_dirs,
                &mut entries,
            );
        }
    }

    entries
}

fn system_autostart_dirs(options: &Options) -> Vec<std::path::PathBuf> {
    let configured = if options.root == std::path::Path::new("/") {
        std::env::var("XDG_CONFIG_DIRS").unwrap_or_else(|_| "/etc/xdg".to_string())
    } else {
        "/etc/xdg".to_string()
    };
    let mut dirs: Vec<_> = configured
        .split(':')
        .filter(|value| value.starts_with('/'))
        .map(|value| rooted(options, value).join("autostart"))
        .collect();
    dirs.dedup();
    dirs
}

fn scan_scope(
    options: &Options,
    principal: &str,
    user_dir: Option<std::path::PathBuf>,
    system_dirs: &[std::path::PathBuf],
    entries: &mut Vec<AutorunEntry>,
) {
    let mut dirs = Vec::new();
    if let Some(user_dir) = user_dir {
        dirs.push(user_dir);
    }
    dirs.extend_from_slice(system_dirs);

    let mut effective_names = HashSet::new();
    for dir in dirs {
        for path in list_files(&options.root, &dir) {
            if path.extension().and_then(|value| value.to_str()) != Some("desktop") {
                continue;
            }
            let Some(content) = read_to_string(&options.root, &path) else {
                continue;
            };
            let file_name = path.file_name().map(|value| value.to_os_string());
            let shadowed = file_name
                .as_ref()
                .map(|name| !effective_names.insert(name.clone()))
                .unwrap_or(false);
            entries.push(parse_desktop_entry(
                options, &path, &content, principal, shadowed,
            ));
        }
    }
}

fn parse_desktop_entry(
    options: &Options,
    path: &std::path::Path,
    content: &str,
    principal: &str,
    shadowed: bool,
) -> AutorunEntry {
    let values = parse_key_values(content);
    let name = values
        .get("Name")
        .cloned()
        .or_else(|| {
            path.file_stem()
                .map(|value| value.to_string_lossy().to_string())
        })
        .unwrap_or_else(|| "desktop autostart".to_string());
    // An empty `Exec=` carries no command, so treat it as absent rather than
    // reporting an empty command/image_path.
    let command = values
        .get("Exec")
        .filter(|value| !value.is_empty())
        .cloned();

    let mut entry = AutorunEntry::new(
        Category::Logon,
        name,
        display_location(path, &options.root),
        path.to_path_buf(),
    );
    entry.description = values.get("Comment").cloned();
    entry.command = command.clone();
    entry.image_path = command.as_deref().and_then(first_command_path);
    entry.timestamp = modified_timestamp(&options.root, path);
    entry.event = Some("graphical user session start".to_string());
    entry.mechanism = Some("XDG autostart desktop entry".to_string());
    entry.principal = Some(principal.to_string());
    entry.target = command.clone();
    entry.completeness = Some("effective XDG file precedence evaluated".to_string());

    let mut notes = Vec::new();
    entry.status = if shadowed {
        notes.push("shadowed by a higher-priority same-name desktop entry".to_string());
        EntryStatus::Shadowed
    } else if values
        .get("Type")
        .map(String::as_str)
        .unwrap_or("Application")
        != "Application"
    {
        notes.push("Type is not Application".to_string());
        EntryStatus::Error
    } else if values
        .get("Hidden")
        .map(|value| value.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
    {
        EntryStatus::Disabled
    } else if command.is_none() {
        notes.push("effective entry has no Exec command".to_string());
        EntryStatus::Error
    } else if let Some(try_exec) = values.get("TryExec") {
        match try_exec_availability(options, principal, try_exec) {
            TryExecAvailability::Available => {
                desktop_environment_status(options, principal, &values, &mut notes)
            }
            TryExecAvailability::Missing => {
                notes.push(format!("TryExec is unavailable: {try_exec}"));
                EntryStatus::Disabled
            }
            TryExecAvailability::Unresolved => {
                notes.push(format!(
                    "TryExec cannot be resolved for an offline or other-user environment: {try_exec}"
                ));
                EntryStatus::Conditional
            }
        }
    } else {
        desktop_environment_status(options, principal, &values, &mut notes)
    };

    if let Some(value) = values.get("OnlyShowIn") {
        notes.push(format!("OnlyShowIn={value}"));
    }
    if let Some(value) = values.get("NotShowIn") {
        notes.push(format!("NotShowIn={value}"));
    }
    if let Some(value) = values.get("NoDisplay") {
        notes.push(format!("NoDisplay={value}"));
    }
    if !notes.is_empty() {
        entry.note = Some(notes.join("; "));
    }

    entry
}

fn desktop_environment_status(
    options: &Options,
    principal: &str,
    values: &HashMap<String, String>,
    notes: &mut Vec<String>,
) -> EntryStatus {
    let only = values.get("OnlyShowIn");
    let excluded = values.get("NotShowIn");
    if only.is_none() && excluded.is_none() {
        return EntryStatus::Enabled;
    }

    if !has_matching_live_environment(options, principal) {
        notes.push(
            "desktop environment is unavailable for an offline or other-user scan; visibility is conditional"
                .to_string(),
        );
        return EntryStatus::Conditional;
    }

    let Ok(current) = std::env::var("XDG_CURRENT_DESKTOP") else {
        notes.push("desktop environment is unknown; visibility is conditional".to_string());
        return EntryStatus::Conditional;
    };
    let desktops: HashSet<&str> = current.split(':').collect();
    let is_listed = |value: &str| {
        value
            .split(';')
            .filter(|item| !item.is_empty())
            .any(|item| desktops.contains(item))
    };
    if only.map(|value| !is_listed(value)).unwrap_or(false)
        || excluded.map(|value| is_listed(value)).unwrap_or(false)
    {
        EntryStatus::Disabled
    } else {
        EntryStatus::Enabled
    }
}

enum TryExecAvailability {
    Available,
    Missing,
    Unresolved,
}

fn try_exec_availability(options: &Options, principal: &str, value: &str) -> TryExecAvailability {
    let candidate = std::path::Path::new(value);
    if candidate.is_absolute() {
        return if is_executable(options, &rooted(options, value)) {
            TryExecAvailability::Available
        } else {
            TryExecAvailability::Missing
        };
    }
    if !has_matching_live_environment(options, principal) {
        return TryExecAvailability::Unresolved;
    }
    let Some(path) = std::env::var_os("PATH") else {
        return TryExecAvailability::Unresolved;
    };
    let mut searched = false;
    for directory in std::env::split_paths(&path).filter(|directory| directory.is_absolute()) {
        searched = true;
        if is_executable(options, &directory.join(candidate)) {
            return TryExecAvailability::Available;
        }
    }
    if searched {
        TryExecAvailability::Missing
    } else {
        TryExecAvailability::Unresolved
    }
}

fn has_matching_live_environment(options: &Options, principal: &str) -> bool {
    options.root == std::path::Path::new("/")
        && std::env::var("USER")
            .or_else(|_| std::env::var("LOGNAME"))
            .map(|current| current == principal)
            .unwrap_or(false)
}

fn is_executable(options: &Options, path: &std::path::Path) -> bool {
    is_executable_file(&options.root, path)
}

fn parse_key_values(content: &str) -> HashMap<String, String> {
    let mut values = HashMap::new();
    let mut in_desktop_entry = false;

    for line in content.lines().map(str::trim) {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            in_desktop_entry = line == "[Desktop Entry]";
            continue;
        }
        if !in_desktop_entry {
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            // `.desktop` files may pad keys/values with spaces around '='.
            values.insert(key.trim().to_string(), value.trim().to_string());
        }
    }

    values
}
