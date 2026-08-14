use std::collections::HashMap;

use crate::{
    cli::Options,
    model::{AutorunEntry, Category, EntryStatus},
};

use super::{
    display_location, first_command_path, list_dirs, list_files, modified_timestamp,
    read_to_string, rooted,
};

pub fn scan_services(options: &Options) -> Vec<AutorunEntry> {
    scan_units(options, "service", Category::Services)
}

pub fn scan_timers(options: &Options) -> Vec<AutorunEntry> {
    scan_units(options, "timer", Category::ScheduledTasks)
}

fn scan_units(options: &Options, extension: &str, category: Category) -> Vec<AutorunEntry> {
    let mut entries = Vec::new();
    for dir in unit_dirs(options) {
        for path in list_files(&dir) {
            if path.extension().and_then(|value| value.to_str()) != Some(extension) {
                continue;
            }
            if let Some(content) = read_to_string(&path) {
                entries.push(parse_unit(options, &path, &content, category));
            }
        }
    }
    entries
}

fn unit_dirs(options: &Options) -> Vec<std::path::PathBuf> {
    let mut dirs = vec![
        rooted(options, "/etc/systemd/system"),
        rooted(options, "/usr/lib/systemd/system"),
        rooted(options, "/lib/systemd/system"),
        rooted(options, "/etc/systemd/user"),
        rooted(options, "/usr/lib/systemd/user"),
        rooted(options, "/lib/systemd/user"),
    ];
    let mut homes = list_dirs(&rooted(options, "/home"));
    if options.root == std::path::Path::new("/") {
        if let Ok(home) = std::env::var("HOME") {
            homes.push(std::path::PathBuf::from(home));
        }
    }
    for home in homes {
        dirs.push(home.join(".config/systemd/user"));
    }
    dirs.sort();
    dirs.dedup();
    dirs
}

fn parse_unit(
    options: &Options,
    path: &std::path::Path,
    content: &str,
    category: Category,
) -> AutorunEntry {
    let values = parse_unit_values(content);
    let name = path
        .file_name()
        .map(|value| value.to_string_lossy().to_string())
        .unwrap_or_else(|| "systemd unit".to_string());
    let command = values
        .get("ExecStart")
        .or_else(|| values.get("ExecStartPre"))
        .or_else(|| values.get("ExecStartPost"))
        .cloned();

    let mut entry = AutorunEntry::new(
        category,
        name,
        display_location(path, &options.root),
        path.to_path_buf(),
    );
    entry.description = values.get("Description").cloned();
    entry.command = command.clone();
    entry.image_path = command.as_deref().and_then(first_command_path);
    entry.timestamp = modified_timestamp(path);
    entry.status = if is_enabled_unit(options, path) {
        EntryStatus::Enabled
    } else {
        EntryStatus::Unknown
    };

    if category == Category::ScheduledTasks {
        entry.note = values
            .get("OnCalendar")
            .or_else(|| values.get("OnBootSec"))
            .or_else(|| values.get("OnUnitActiveSec"))
            .map(|value| format!("timer={value}"));
    } else if let Some(wanted_by) = values.get("WantedBy") {
        entry.note = Some(format!("WantedBy={wanted_by}"));
    }

    entry
}

fn parse_unit_values(content: &str) -> HashMap<String, String> {
    let mut values = HashMap::new();
    for line in content.lines().map(str::trim) {
        if line.is_empty() || line.starts_with('#') || line.starts_with('[') {
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            // systemd INI semantics: a later assignment overrides an earlier one.
            // Keys and values may be padded with spaces around the '='.
            values.insert(key.trim().to_string(), value.trim().to_string());
        }
    }
    values
}

fn is_enabled_unit(options: &Options, path: &std::path::Path) -> bool {
    let Some(file_name) = path.file_name() else {
        return false;
    };

    // Enablement symlinks live in `*.wants` directories. For units shipped under
    // /usr/lib or /lib, those symlinks are created under /etc/systemd/system,
    // so both the unit's own directory and the canonical enablement root must be
    // searched.
    let mut bases: Vec<std::path::PathBuf> = Vec::new();
    if let Some(parent) = path.parent() {
        bases.push(parent.to_path_buf());
    }
    let etc = rooted(options, "/etc/systemd/system");
    if !bases.contains(&etc) {
        bases.push(etc);
    }

    for base in bases {
        for dir in list_dirs(&base) {
            if dir
                .extension()
                .and_then(|value| value.to_str())
                .map(|value| value.ends_with("wants"))
                .unwrap_or(false)
                && dir.join(file_name).exists()
            {
                return true;
            }
        }
    }
    false
}
