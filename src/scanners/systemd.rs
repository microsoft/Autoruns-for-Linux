use std::collections::{HashMap, HashSet};

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
    let dirs = unit_dirs(options);
    // Walk every `*.wants` directory once up front so enablement is an O(1)
    // lookup per unit instead of re-listing those directories for each unit.
    let enabled = enabled_unit_names(options, &dirs);
    let mut entries = Vec::new();
    for dir in &dirs {
        for path in list_files(&options.root, dir) {
            if path.extension().and_then(|value| value.to_str()) != Some(extension) {
                continue;
            }
            if let Some(content) = read_to_string(&options.root, &path) {
                entries.push(parse_unit(options, &path, &content, category, &enabled));
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
    let mut homes = list_dirs(&options.root, &rooted(options, "/home"));
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
    enabled: &HashSet<std::ffi::OsString>,
) -> AutorunEntry {
    let values = parse_unit_values(content);
    let name = path
        .file_name()
        .map(|value| value.to_string_lossy().to_string())
        .unwrap_or_else(|| "systemd unit".to_string());
    let command = values
        .get("ExecStart")
        .filter(|value| !value.is_empty())
        .or_else(|| values.get("ExecStartPre").filter(|value| !value.is_empty()))
        .or_else(|| {
            values
                .get("ExecStartPost")
                .filter(|value| !value.is_empty())
        })
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
    entry.timestamp = modified_timestamp(&options.root, path);
    entry.status = if path
        .file_name()
        .map(|name| enabled.contains(name))
        .unwrap_or(false)
    {
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
            let key = key.trim();
            let value = value.trim();
            // Keys and values may be padded with spaces around the '='.
            if key.starts_with("ExecStart") {
                // systemd runs multiple ExecStart*/ExecStartPre/ExecStartPost
                // assignments in sequence, so accumulate them instead of
                // dropping earlier commands. An empty assignment resets the
                // list, matching systemd semantics.
                if value.is_empty() {
                    values.insert(key.to_string(), String::new());
                } else {
                    values
                        .entry(key.to_string())
                        .and_modify(|existing: &mut String| {
                            if existing.is_empty() {
                                existing.push_str(value);
                            } else {
                                existing.push_str("; ");
                                existing.push_str(value);
                            }
                        })
                        .or_insert_with(|| value.to_string());
                }
            } else {
                // systemd INI semantics: a later assignment overrides an
                // earlier one for ordinary keys.
                values.insert(key.to_string(), value.to_string());
            }
        }
    }
    values
}

// Collects, once per scan, the file names that are enablement symlinks in any
// `*.wants` directory under the scanned unit directories plus the canonical
// enablement roots (/etc/systemd/system and /etc/systemd/user). Enablement is
// represented specifically by symlinks, so regular files copied into a `*.wants`
// directory are ignored.
fn enabled_unit_names(
    options: &Options,
    dirs: &[std::path::PathBuf],
) -> HashSet<std::ffi::OsString> {
    let mut bases: Vec<std::path::PathBuf> = dirs.to_vec();
    for root in ["/etc/systemd/system", "/etc/systemd/user"] {
        let base = rooted(options, root);
        if !bases.contains(&base) {
            bases.push(base);
        }
    }

    let mut names = HashSet::new();
    for base in bases {
        for dir in list_dirs(&options.root, &base) {
            let is_wants = dir
                .extension()
                .and_then(|value| value.to_str())
                .map(|value| value.ends_with("wants"))
                .unwrap_or(false);
            if !is_wants {
                continue;
            }
            if let Ok(read_dir) = std::fs::read_dir(&dir) {
                for entry in read_dir.flatten() {
                    if entry
                        .file_type()
                        .map(|kind| kind.is_symlink())
                        .unwrap_or(false)
                    {
                        names.insert(entry.file_name());
                    }
                }
            }
        }
    }
    names
}
