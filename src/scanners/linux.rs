use std::collections::HashSet;

use crate::{
    cli::Options,
    model::{AutorunEntry, Category, EntryStatus},
};

use super::{
    directory_identity, display_location, in_root_path, is_executable_file, list_dirs, list_files,
    modified_timestamp, path_is_file, read_link_in_root, read_to_string, resolve_in_root, rooted,
};

pub fn scan_modules(options: &Options) -> Vec<AutorunEntry> {
    let mut entries = Vec::new();
    let modules = rooted(options, "/etc/modules");
    if let Some(content) = read_to_string(&options.root, &modules) {
        parse_module_file(options, &modules, &content, &mut entries);
    }

    let mut names = HashSet::new();
    let mut physical_dirs = HashSet::new();
    for dir_name in [
        "/etc/modules-load.d",
        "/run/modules-load.d",
        "/usr/local/lib/modules-load.d",
        "/usr/lib/modules-load.d",
        "/lib/modules-load.d",
    ] {
        let dir = rooted(options, dir_name);
        if let Some(identity) = directory_identity(&options.root, &dir) {
            if !physical_dirs.insert(identity) {
                continue;
            }
        }
        for path in list_files(&options.root, &dir) {
            if path.extension().and_then(|value| value.to_str()) != Some("conf") {
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
                    Category::Services,
                    path.file_name()
                        .map(|value| value.to_string_lossy().to_string())
                        .unwrap_or_else(|| "modules-load mask".to_string()),
                    display_location(&path, &options.root),
                    path.clone(),
                );
                entry.status = EntryStatus::Disabled;
                entry.event = Some("system boot".to_string());
                entry.mechanism = Some("modules-load.d mask".to_string());
                entry.note = Some("higher-priority /dev/null mask".to_string());
                entry.completeness =
                    Some("effective modules-load precedence evaluated".to_string());
                entries.push(entry);
                continue;
            }
            if let Some(content) = read_to_string(&options.root, &path) {
                parse_module_file(options, &path, &content, &mut entries);
            }
        }
    }
    entries
}

fn parse_module_file(
    options: &Options,
    path: &std::path::Path,
    content: &str,
    entries: &mut Vec<AutorunEntry>,
) {
    for (line_number, line) in content.lines().enumerate() {
        let module = line.split(['#', ';']).next().unwrap_or_default().trim();
        if module.is_empty() {
            continue;
        }
        let mut entry = AutorunEntry::new(
            Category::Services,
            module,
            format!(
                "{}:{}",
                display_location(path, &options.root),
                line_number + 1
            ),
            path.to_path_buf(),
        );
        entry.status = EntryStatus::Enabled;
        entry.timestamp = modified_timestamp(&options.root, path);
        entry.note = Some("effective kernel module load configuration".to_string());
        entry.event = Some("system boot".to_string());
        entry.mechanism = Some("kernel module load configuration".to_string());
        entry.target = Some(module.to_string());
        entry.completeness = Some("effective modules-load precedence evaluated".to_string());
        entries.push(entry);
    }
}

pub fn scan_boot(options: &Options) -> Vec<AutorunEntry> {
    let mut entries = Vec::new();
    for path in [
        rooted(options, "/etc/rc.local"),
        rooted(options, "/etc/init.d/rc.local"),
    ] {
        if path_is_file(&options.root, &path) {
            entries.push(script_entry(
                options,
                Category::Boot,
                &path,
                "system boot",
                "rc.local",
                "rc.local compatibility hook",
                is_executable_file(&options.root, &path),
            ));
            break;
        }
    }
    let enabled = sysv_enabled_names(options);
    for script in list_files(&options.root, &rooted(options, "/etc/init.d")) {
        if script.file_name().and_then(|name| name.to_str()) == Some("rc.local") {
            continue;
        }
        let name = script
            .file_name()
            .map(|value| value.to_string_lossy().to_string())
            .unwrap_or_else(|| "init script".to_string());
        let executable = is_executable_file(&options.root, &script);
        let mut entry = script_entry(
            options,
            Category::Boot,
            &script,
            "runlevel transition",
            "SysV init",
            "SysV init script",
            executable && enabled.contains(&name),
        );
        entry.status = if !executable {
            EntryStatus::Disabled
        } else if enabled.contains(&name) {
            EntryStatus::Enabled
        } else {
            EntryStatus::Unknown
        };
        entries.push(entry);
    }
    entries
}

pub fn scan_hijacks(options: &Options) -> Vec<AutorunEntry> {
    let mut entries = Vec::new();

    for path in list_files(&options.root, &rooted(options, "/etc/alternatives")) {
        let mut entry = AutorunEntry::new(
            Category::Hijacks,
            path.file_name()
                .map(|value| value.to_string_lossy().to_string())
                .unwrap_or_else(|| "alternative".to_string()),
            display_location(&path, &options.root),
            path.clone(),
        );
        if let Some(resolved) = resolve_in_root(&options.root, &path) {
            let target = in_root_path(&resolved, &options.root);
            entry.image_path = Some(target.clone());
            entry.target = Some(target.display().to_string());
        }
        entry.status = EntryStatus::Conditional;
        entry.timestamp = modified_timestamp(&options.root, &path);
        entry.event = Some("command invocation".to_string());
        entry.mechanism = Some("alternatives symlink".to_string());
        entry.note = Some("resolved alternatives-managed command target".to_string());
        entry.completeness = Some("effective symlink target resolved".to_string());
        entries.push(entry);
    }

    entries
}

pub fn scan_loader(options: &Options) -> Vec<AutorunEntry> {
    let mut entries = Vec::new();
    let preload = rooted(options, "/etc/ld.so.preload");
    if let Some(content) = read_to_string(&options.root, &preload) {
        for (line_number, line) in content.lines().enumerate() {
            let values = line.split('#').next().unwrap_or_default();
            for value in values.split_whitespace() {
                let mut entry = AutorunEntry::new(
                    Category::Loader,
                    value,
                    format!(
                        "{}:{}",
                        display_location(&preload, &options.root),
                        line_number + 1
                    ),
                    preload.clone(),
                );
                entry.image_path = Some(std::path::PathBuf::from(value));
                entry.status = EntryStatus::Enabled;
                entry.timestamp = modified_timestamp(&options.root, &preload);
                entry.event = Some("dynamic process load".to_string());
                entry.mechanism = Some("ld.so preload".to_string());
                entry.target = Some(value.to_string());
                entry.note = Some("dynamic loader preload module".to_string());
                entry.completeness =
                    Some("all whitespace-delimited preload objects parsed".to_string());
                entries.push(entry);
            }
        }
    }

    let main_config = rooted(options, "/etc/ld.so.conf");
    let mut pending = vec![(main_config.clone(), true)];
    pending.extend(
        list_files(&options.root, &rooted(options, "/etc/ld.so.conf.d"))
            .into_iter()
            .map(|path| (path, false)),
    );
    let mut seen = HashSet::new();
    while let Some((file, included)) = pending.pop() {
        if !seen.insert(file.clone()) {
            continue;
        }
        if !included && file.extension().and_then(|value| value.to_str()) != Some("conf") {
            continue;
        }
        let Some(content) = read_to_string(&options.root, &file) else {
            continue;
        };
        for (line_number, line) in content.lines().enumerate() {
            let value = line.split('#').next().unwrap_or_default().trim();
            if value.is_empty() {
                continue;
            }
            if let Some(patterns) = value
                .strip_prefix("include")
                .filter(|rest| rest.chars().next().is_some_and(char::is_whitespace))
            {
                for pattern in patterns.split_whitespace() {
                    let matches = expand_linker_include(options, &file, pattern);
                    let mut entry = AutorunEntry::new(
                        Category::Loader,
                        pattern,
                        format!(
                            "{}:{}",
                            display_location(&file, &options.root),
                            line_number + 1
                        ),
                        file.clone(),
                    );
                    entry.status = EntryStatus::Conditional;
                    entry.timestamp = modified_timestamp(&options.root, &file);
                    entry.event = Some("dynamic linker configuration load".to_string());
                    entry.mechanism = Some("ld.so.conf include".to_string());
                    entry.target = Some(pattern.to_string());
                    entry.note = Some(format!(
                        "matched {} in-root configuration file(s)",
                        matches.len()
                    ));
                    entry.completeness = Some("include glob expanded inside scan root".to_string());
                    entries.push(entry);
                    pending.extend(matches.into_iter().map(|path| (path, true)));
                }
                continue;
            }
            let mut entry = AutorunEntry::new(
                Category::Loader,
                value,
                format!(
                    "{}:{}",
                    display_location(&file, &options.root),
                    line_number + 1
                ),
                file.clone(),
            );
            entry.status = EntryStatus::Conditional;
            entry.timestamp = modified_timestamp(&options.root, &file);
            entry.event = Some("dynamic library resolution".to_string());
            entry.mechanism = Some("dynamic linker search path".to_string());
            entry.target = Some(value.to_string());
            entry.note =
                Some("configuration-only search directory; not itself a loaded module".to_string());
            entry.completeness = Some("parsed configured search path".to_string());
            entries.push(entry);
        }
    }

    entries
}

fn expand_linker_include(
    options: &Options,
    source: &std::path::Path,
    pattern: &str,
) -> Vec<std::path::PathBuf> {
    let pattern_path = if std::path::Path::new(pattern).is_absolute() {
        rooted(options, pattern)
    } else {
        source.parent().unwrap_or(&options.root).join(pattern)
    };
    let in_image = in_root_path(&pattern_path, &options.root);
    let components: Vec<_> = in_image
        .components()
        .filter_map(|component| match component {
            std::path::Component::Normal(value) => Some(value.to_os_string()),
            std::path::Component::ParentDir => Some(std::ffi::OsString::from("..")),
            _ => None,
        })
        .collect();
    let mut candidates = vec![options.root.clone()];
    for (index, component) in components.iter().enumerate() {
        if component.as_os_str() == std::ffi::OsStr::new("..") {
            for candidate in &mut candidates {
                if candidate != &options.root {
                    candidate.pop();
                }
            }
            continue;
        }
        let text = component.to_string_lossy();
        let has_meta = text.chars().any(|value| matches!(value, '*' | '?' | '['));
        let last = index + 1 == components.len();
        let mut next = Vec::new();
        for candidate in candidates {
            if has_meta {
                let Ok(pattern) = glob::Pattern::new(&text) else {
                    continue;
                };
                let children = if last {
                    list_files(&options.root, &candidate)
                } else {
                    list_dirs(&options.root, &candidate)
                };
                next.extend(children.into_iter().filter(|child| {
                    child
                        .file_name()
                        .map(|name| pattern.matches(&name.to_string_lossy()))
                        .unwrap_or(false)
                }));
            } else {
                next.push(candidate.join(component));
            }
        }
        candidates = next;
    }
    candidates.retain(|path| path_is_file(&options.root, path));
    candidates.sort();
    candidates.dedup();
    candidates
}

pub fn scan_network(options: &Options) -> Vec<AutorunEntry> {
    let mut entries = Vec::new();
    for dir in [
        "/etc/NetworkManager/dispatcher.d",
        "/etc/NetworkManager/dispatcher.d/pre-up.d",
        "/etc/NetworkManager/dispatcher.d/pre-down.d",
        "/etc/NetworkManager/dispatcher.d/no-wait.d",
        "/etc/network/if-up.d",
        "/etc/network/if-down.d",
        "/etc/dhcp/dhclient-enter-hooks.d",
        "/etc/dhcp/dhclient-exit-hooks.d",
    ] {
        for script in list_files(&options.root, &rooted(options, dir)) {
            if !eligible_hook_name(&script) || !is_executable_file(&options.root, &script) {
                continue;
            }
            entries.push(script_entry(
                options,
                Category::Network,
                &script,
                "network state change",
                "network dispatcher hook",
                "eligible executable network hook",
                true,
            ));
        }
    }
    entries
}

fn script_entry(
    options: &Options,
    category: Category,
    path: &std::path::Path,
    event: &str,
    mechanism: &str,
    note: &str,
    enabled: bool,
) -> AutorunEntry {
    let mut entry = AutorunEntry::new(
        category,
        path.file_name()
            .map(|value| value.to_string_lossy().to_string())
            .unwrap_or_else(|| "startup script".to_string()),
        display_location(path, &options.root),
        path.to_path_buf(),
    );
    let in_image = in_root_path(path, &options.root);
    entry.command = Some(in_image.display().to_string());
    entry.image_path = Some(in_image.clone());
    entry.status = if enabled {
        EntryStatus::Enabled
    } else {
        EntryStatus::Disabled
    };
    entry.timestamp = modified_timestamp(&options.root, path);
    entry.event = Some(event.to_string());
    entry.mechanism = Some(mechanism.to_string());
    entry.activating_entity = Some(display_location(
        path.parent().unwrap_or(path),
        &options.root,
    ));
    entry.target = Some(in_image.display().to_string());
    entry.note = Some(note.to_string());
    entry.completeness = Some("eligibility evaluated".to_string());
    entry
}

fn sysv_enabled_names(options: &Options) -> HashSet<String> {
    let mut names = HashSet::new();
    for runlevel in ["0", "1", "2", "3", "4", "5", "6", "S"] {
        for path in list_files(
            &options.root,
            &rooted(options, &format!("/etc/rc{runlevel}.d")),
        ) {
            let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            if name.len() > 3
                && name.starts_with('S')
                && name[1..3].chars().all(|value| value.is_ascii_digit())
            {
                names.insert(name[3..].to_string());
            }
        }
    }
    names
}

fn eligible_hook_name(path: &std::path::Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .map(|name| {
            !name.is_empty()
                && name
                    .chars()
                    .all(|value| value.is_ascii_alphanumeric() || matches!(value, '_' | '-'))
        })
        .unwrap_or(false)
}

fn is_dev_null_mask(root: &std::path::Path, path: &std::path::Path) -> bool {
    read_link_in_root(root, path)
        .map(|target| target == std::path::Path::new("/dev/null"))
        .unwrap_or(false)
}
