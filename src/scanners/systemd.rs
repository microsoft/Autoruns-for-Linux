use std::collections::HashMap;

use crate::{cli::Options, model::{AutorunEntry, Category, EntryStatus}};

use super::{display_location, first_command_path, list_dirs, list_files, modified_timestamp, read_to_string, rooted};

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
    ];
    for home in list_dirs(&rooted(options, "/home")) {
        dirs.push(home.join(".config/systemd/user"));
    }
    dirs
}

fn parse_unit(options: &Options, path: &std::path::Path, content: &str, category: Category) -> AutorunEntry {
    let values = parse_unit_values(content);
    let name = path.file_name().map(|value| value.to_string_lossy().to_string()).unwrap_or_else(|| "systemd unit".to_string());
    let command = values
        .get("ExecStart")
        .or_else(|| values.get("ExecStartPre"))
        .or_else(|| values.get("ExecStartPost"))
        .cloned();

    let mut entry = AutorunEntry::new(category, name, display_location(path, &options.root), path.to_path_buf());
    entry.description = values.get("Description").cloned();
    entry.command = command.clone();
    entry.image_path = command.as_deref().and_then(first_command_path);
    entry.timestamp = modified_timestamp(path);
    entry.status = if is_enabled_unit(path) { EntryStatus::Enabled } else { EntryStatus::Unknown };

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
            values.entry(key.to_string()).or_insert_with(|| value.to_string());
        }
    }
    values
}

fn is_enabled_unit(path: &std::path::Path) -> bool {
    let Some(file_name) = path.file_name() else {
        return false;
    };
    let Some(parent) = path.parent() else {
        return false;
    };
    for dir in list_dirs(parent) {
        if dir.extension().and_then(|value| value.to_str()).map(|value| value.ends_with("wants")).unwrap_or(false) {
            if dir.join(file_name).exists() {
                return true;
            }
        }
    }
    false
}