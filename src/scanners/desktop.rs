use std::collections::HashMap;

use crate::{
    cli::Options,
    model::{AutorunEntry, Category, EntryStatus},
};

use super::{
    display_location, first_command_path, list_dirs, list_files, modified_timestamp,
    read_to_string, rooted,
};

pub fn scan(options: &Options) -> Vec<AutorunEntry> {
    let mut entries = Vec::new();
    let mut dirs = vec![rooted(options, "/etc/xdg/autostart")];

    for home in list_dirs(&rooted(options, "/home")) {
        dirs.push(home.join(".config/autostart"));
    }

    if options.root == std::path::Path::new("/") {
        if let Ok(home) = std::env::var("HOME") {
            dirs.push(std::path::PathBuf::from(home).join(".config/autostart"));
        }
    }

    for dir in dirs {
        for path in list_files(&dir) {
            if path.extension().and_then(|value| value.to_str()) != Some("desktop") {
                continue;
            }
            if let Some(content) = read_to_string(&path) {
                entries.push(parse_desktop_entry(options, &path, &content));
            }
        }
    }

    entries
}

fn parse_desktop_entry(options: &Options, path: &std::path::Path, content: &str) -> AutorunEntry {
    let values = parse_key_values(content);
    let name = values
        .get("Name")
        .cloned()
        .or_else(|| {
            path.file_stem()
                .map(|value| value.to_string_lossy().to_string())
        })
        .unwrap_or_else(|| "desktop autostart".to_string());
    let command = values.get("Exec").cloned();

    let mut entry = AutorunEntry::new(
        Category::Logon,
        name,
        display_location(path, &options.root),
        path.to_path_buf(),
    );
    entry.description = values.get("Comment").cloned();
    entry.command = command.clone();
    entry.image_path = command.as_deref().and_then(first_command_path);
    entry.timestamp = modified_timestamp(path);
    entry.status = if values
        .get("Hidden")
        .map(|value| value.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
    {
        EntryStatus::Disabled
    } else {
        EntryStatus::Enabled
    };

    let mut notes = Vec::new();
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
            values.insert(key.to_string(), value.to_string());
        }
    }

    values
}
