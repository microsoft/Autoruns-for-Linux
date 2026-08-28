use std::collections::HashSet;

use crate::{
    cli::Options,
    model::{AutorunEntry, Category, EntryStatus},
};

use super::{
    directory_identity, display_location, first_command_path, in_root_path, is_executable_file,
    list_dirs, list_files, modified_timestamp, read_link_in_root, read_to_string, rooted,
};

pub fn scan(options: &Options) -> Vec<AutorunEntry> {
    let mut entries = scan_udev(options);
    entries.extend(scan_fstab(options));
    entries.extend(scan_autofs(options));
    entries.extend(scan_media_evidence(options));
    entries
}

fn scan_udev(options: &Options) -> Vec<AutorunEntry> {
    let mut entries = Vec::new();
    let mut names = HashSet::new();
    let mut physical_dirs = HashSet::new();
    for dir_name in [
        "/etc/udev/rules.d",
        "/run/udev/rules.d",
        "/usr/local/lib/udev/rules.d",
        "/usr/lib/udev/rules.d",
        "/lib/udev/rules.d",
    ] {
        let dir = rooted(options, dir_name);
        if let Some(identity) = directory_identity(&options.root, &dir) {
            if !physical_dirs.insert(identity) {
                continue;
            }
        }
        for path in list_files(&options.root, &dir) {
            if path.extension().and_then(|value| value.to_str()) != Some("rules") {
                continue;
            }
            let Some(name) = path.file_name().map(|value| value.to_os_string()) else {
                continue;
            };
            if !names.insert(name) {
                continue;
            }
            if is_dev_null_mask(&options.root, &path) {
                let mut entry = AutorunEntry::new(
                    Category::DeviceMount,
                    path.file_name()
                        .map(|value| value.to_string_lossy().to_string())
                        .unwrap_or_else(|| "udev mask".to_string()),
                    display_location(&path, &options.root),
                    path.clone(),
                );
                entry.status = EntryStatus::Disabled;
                entry.event = Some("device event".to_string());
                entry.mechanism = Some("udev rules file mask".to_string());
                entry.note = Some("higher-priority /dev/null mask".to_string());
                entry.completeness = Some("effective udev precedence evaluated".to_string());
                entries.push(entry);
                continue;
            }
            if let Some(content) = read_to_string(&options.root, &path) {
                parse_udev_rules(options, &path, &content, &mut entries);
            }
        }
    }
    entries
}

fn parse_udev_rules(
    options: &Options,
    path: &std::path::Path,
    content: &str,
    entries: &mut Vec<AutorunEntry>,
) {
    for (line_number, line) in logical_lines(content).into_iter().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let fields: Vec<_> = split_quoted(line, ',')
            .into_iter()
            .filter_map(|field| parse_assignment(&field))
            .collect();
        let conditions = fields
            .iter()
            .filter(|(key, operator, _)| {
                matches!(operator.as_str(), "==" | "!=") && !key.starts_with("PROGRAM")
            })
            .map(|(key, operator, value)| format!("{key}{operator}\"{value}\""))
            .collect::<Vec<_>>()
            .join(", ");
        for (key, _, value) in &fields {
            let action = if key == "PROGRAM" {
                Some(("udev PROGRAM match", false))
            } else if key == "IMPORT{program}" {
                Some(("udev IMPORT{program}", false))
            } else if key.starts_with("RUN{builtin}") {
                Some(("udev RUN builtin", true))
            } else if key == "RUN" || key.starts_with("RUN{program}") {
                Some(("udev RUN{program}", false))
            } else if matches!(
                key.as_str(),
                "SYSTEMD_WANTS"
                    | "SYSTEMD_USER_WANTS"
                    | "ENV{SYSTEMD_WANTS}"
                    | "ENV{SYSTEMD_USER_WANTS}"
            ) {
                Some(("udev systemd wants", true))
            } else {
                None
            };
            let Some((mechanism, unit_target)) = action else {
                continue;
            };
            let targets: Vec<&str> = if unit_target {
                value.split_whitespace().collect()
            } else {
                vec![value]
            };
            for target in targets {
                let mut entry = AutorunEntry::new(
                    Category::DeviceMount,
                    format!(
                        "{} line {}",
                        path.file_name()
                            .map(|value| value.to_string_lossy())
                            .unwrap_or_default(),
                        line_number + 1
                    ),
                    format!(
                        "{}:{}",
                        display_location(path, &options.root),
                        line_number + 1
                    ),
                    path.to_path_buf(),
                );
                entry.status = EntryStatus::Conditional;
                entry.timestamp = modified_timestamp(&options.root, path);
                entry.event = Some(if conditions.is_empty() {
                    "matching device event".to_string()
                } else {
                    conditions.clone()
                });
                entry.mechanism = Some(mechanism.to_string());
                entry.principal = Some(
                    if key.contains("USER_WANTS") {
                        "matching user manager"
                    } else {
                        "system"
                    }
                    .to_string(),
                );
                entry.activating_entity = Some(display_location(path, &options.root));
                entry.target = Some(target.to_string());
                if !unit_target {
                    entry.command = Some(target.to_string());
                    entry.image_path = first_command_path(target);
                }
                entry.note =
                    Some("static rule evidence; device event was not synthesized".to_string());
                entry.completeness = Some("effective udev rule action parsed".to_string());
                entries.push(entry);
            }
        }
    }
}

fn scan_fstab(options: &Options) -> Vec<AutorunEntry> {
    let path = rooted(options, "/etc/fstab");
    let Some(content) = read_to_string(&options.root, &path) else {
        return Vec::new();
    };
    let mut entries = Vec::new();
    for (line_number, line) in content.lines().enumerate() {
        let line = line.split('#').next().unwrap_or_default().trim();
        if line.is_empty() {
            continue;
        }
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() < 4 {
            continue;
        }
        let source = unescape_fstab(fields[0]);
        let mount_point = unescape_fstab(fields[1]);
        let options_field = fields[3];
        let options_list: Vec<&str> = options_field.split(',').collect();
        let relationships: Vec<&str> = options_list
            .iter()
            .copied()
            .filter(|value| value.starts_with("x-systemd."))
            .collect();
        if relationships.is_empty() {
            continue;
        }
        let mut entry = AutorunEntry::new(
            Category::DeviceMount,
            format!("fstab mount {mount_point}"),
            format!(
                "{}:{}",
                display_location(&path, &options.root),
                line_number + 1
            ),
            path.clone(),
        );
        entry.status = if options_list.contains(&"noauto") {
            EntryStatus::Disabled
        } else {
            EntryStatus::Conditional
        };
        entry.event = Some(if options_list.contains(&"x-systemd.automount") {
            format!("path access at {mount_point}")
        } else {
            "system boot or explicit mount request".to_string()
        });
        entry.mechanism = Some("fstab systemd generator relationship".to_string());
        entry.principal = Some("system".to_string());
        entry.activating_entity = Some(source);
        entry.target = Some(mount_point);
        entry.note = Some(relationships.join("; "));
        entry.completeness = Some("x-systemd fstab options parsed".to_string());
        entries.push(entry);
    }
    entries
}

fn scan_autofs(options: &Options) -> Vec<AutorunEntry> {
    let mut entries = Vec::new();
    let mut masters = vec![rooted(options, "/etc/auto.master")];
    masters.extend(list_files(
        &options.root,
        &rooted(options, "/etc/auto.master.d"),
    ));
    for path in masters {
        if path != rooted(options, "/etc/auto.master")
            && path.extension().and_then(|value| value.to_str()) != Some("autofs")
        {
            continue;
        }
        let Some(content) = read_to_string(&options.root, &path) else {
            continue;
        };
        for (line_number, line) in content.lines().enumerate() {
            let line = line.split('#').next().unwrap_or_default().trim();
            if line.is_empty() || line.starts_with('+') {
                continue;
            }
            let fields: Vec<&str> = line.split_whitespace().collect();
            if fields.len() < 2 {
                continue;
            }
            let map = fields[1];
            let command = map
                .strip_prefix("program:")
                .or_else(|| map.strip_prefix('|'));
            let mut entry = AutorunEntry::new(
                Category::DeviceMount,
                format!("autofs map for {}", fields[0]),
                format!(
                    "{}:{}",
                    display_location(&path, &options.root),
                    line_number + 1
                ),
                path.clone(),
            );
            entry.status = EntryStatus::Conditional;
            entry.event = Some(format!("path access under {}", fields[0]));
            entry.mechanism = Some(
                if command.is_some() {
                    "autofs executable program map"
                } else {
                    "autofs map"
                }
                .to_string(),
            );
            entry.principal = Some("system".to_string());
            entry.activating_entity = Some("autofs".to_string());
            entry.target = Some(map.to_string());
            if let Some(command) = command {
                entry.command = Some(command.to_string());
                entry.image_path = first_command_path(command);
            }
            entry.note = Some("static map evidence; no path lookup was triggered".to_string());
            entry.completeness = Some("autofs master map parsed".to_string());
            entries.push(entry);
        }
    }
    entries
}

fn scan_media_evidence(options: &Options) -> Vec<AutorunEntry> {
    let mut entries = Vec::new();
    for base_name in ["/media", "/run/media", "/mnt"] {
        let base = rooted(options, base_name);
        let mut mounts = list_dirs(&options.root, &base);
        for first in mounts.clone() {
            mounts.extend(list_dirs(&options.root, &first));
        }
        mounts.sort();
        mounts.dedup();
        for mount in mounts {
            for path in list_files(&options.root, &mount) {
                let name = path
                    .file_name()
                    .and_then(|value| value.to_str())
                    .unwrap_or("");
                if !matches!(name, ".autorun" | "autorun" | "autorun.sh" | ".autoopen") {
                    continue;
                }
                let mut entry = AutorunEntry::new(
                    Category::DeviceMount,
                    name,
                    display_location(&path, &options.root),
                    path.clone(),
                );
                entry.status = EntryStatus::Conditional;
                entry.timestamp = modified_timestamp(&options.root, &path);
                entry.event = Some("removable media mount and desktop inspection".to_string());
                entry.mechanism = Some("mounted-media autorun/autoopen evidence".to_string());
                entry.activating_entity = Some(display_location(&mount, &options.root));
                let target = in_root_path(&path, &options.root);
                entry.target = Some(target.display().to_string());
                if is_executable_file(&options.root, &path) {
                    entry.command = Some(target.display().to_string());
                    entry.image_path = Some(target);
                }
                entry.note = Some("evidence only; media was not mounted or executed".to_string());
                entry.completeness =
                    Some("mounted media roots inspected to two levels".to_string());
                entries.push(entry);
            }
        }
    }
    entries
}

fn split_quoted(value: &str, separator: char) -> Vec<String> {
    let mut values = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    let mut escaped = false;
    for character in value.chars() {
        if escaped {
            current.push(character);
            escaped = false;
        } else if character == '\\' {
            current.push(character);
            escaped = true;
        } else if character == '"' {
            current.push(character);
            quoted = !quoted;
        } else if character == separator && !quoted {
            values.push(std::mem::take(&mut current));
        } else {
            current.push(character);
        }
    }
    values.push(current);
    values
}

fn parse_assignment(value: &str) -> Option<(String, String, String)> {
    for operator in ["==", "!=", "+=", ":=", "="] {
        if let Some((key, value)) = value.split_once(operator) {
            return Some((
                key.trim().to_string(),
                operator.to_string(),
                value.trim().trim_matches('"').to_string(),
            ));
        }
    }
    None
}

fn logical_lines(content: &str) -> Vec<String> {
    let mut lines = Vec::new();
    let mut current = String::new();
    for line in content.lines() {
        let line = line.trim_end();
        let continued = line.ends_with('\\');
        let line = line.strip_suffix('\\').unwrap_or(line);
        if !current.is_empty() {
            current.push(' ');
        }
        current.push_str(line.trim());
        if !continued {
            lines.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        lines.push(current);
    }
    lines
}

fn unescape_fstab(value: &str) -> String {
    value
        .replace("\\040", " ")
        .replace("\\011", "\t")
        .replace("\\134", "\\")
}

fn is_dev_null_mask(root: &std::path::Path, path: &std::path::Path) -> bool {
    read_link_in_root(root, path)
        .map(|target| target == std::path::Path::new("/dev/null"))
        .unwrap_or(false)
}
