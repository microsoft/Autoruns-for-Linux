use crate::{
    cli::Options,
    model::{AutorunEntry, Category, EntryStatus},
};

use super::{
    display_location, in_root_path, list_files, modified_timestamp, read_to_string,
    resolve_in_root, rooted,
};

pub fn scan_modules(options: &Options) -> Vec<AutorunEntry> {
    let mut entries = Vec::new();
    for path in [rooted(options, "/etc/modules")]
        .into_iter()
        .chain(list_files(
            &options.root,
            &rooted(options, "/etc/modules-load.d"),
        ))
    {
        if let Some(content) = read_to_string(&options.root, &path) {
            for (line_number, line) in content.lines().enumerate() {
                let module = line.trim();
                if module.is_empty() || module.starts_with('#') {
                    continue;
                }
                let mut entry = AutorunEntry::new(
                    Category::Services,
                    module.to_string(),
                    format!(
                        "{}:{}",
                        display_location(&path, &options.root),
                        line_number + 1
                    ),
                    path.clone(),
                );
                entry.status = EntryStatus::Enabled;
                entry.timestamp = modified_timestamp(&options.root, &path);
                entry.note = Some("kernel module load configuration".to_string());
                entries.push(entry);
            }
        }
    }
    entries
}

pub fn scan_boot(options: &Options) -> Vec<AutorunEntry> {
    let mut entries = Vec::new();
    for path in [
        rooted(options, "/etc/rc.local"),
        rooted(options, "/etc/init.d/rc.local"),
    ] {
        if resolve_in_root(&options.root, &path)
            .map(|resolved| resolved.exists())
            .unwrap_or(false)
        {
            let mut entry = AutorunEntry::new(
                Category::Boot,
                "rc.local",
                display_location(&path, &options.root),
                path.clone(),
            );
            let in_image = in_root_path(&path, &options.root);
            entry.command = Some(in_image.display().to_string());
            entry.image_path = Some(in_image);
            entry.status = EntryStatus::Enabled;
            entry.timestamp = modified_timestamp(&options.root, &path);
            entries.push(entry);
        }
    }
    for script in list_files(&options.root, &rooted(options, "/etc/init.d")) {
        let mut entry = AutorunEntry::new(
            Category::Boot,
            script
                .file_name()
                .map(|value| value.to_string_lossy().to_string())
                .unwrap_or_else(|| "init script".to_string()),
            display_location(&script, &options.root),
            script.clone(),
        );
        let in_image = in_root_path(&script, &options.root);
        entry.command = Some(in_image.display().to_string());
        entry.image_path = Some(in_image);
        entry.status = EntryStatus::Unknown;
        entry.timestamp = modified_timestamp(&options.root, &script);
        entry.note = Some("SysV init script; enabled state depends on rc.d links".to_string());
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
        entry.status = EntryStatus::Unknown;
        entry.timestamp = modified_timestamp(&options.root, &path);
        entry.note = Some("alternatives-managed command target".to_string());
        entries.push(entry);
    }

    entries
}

pub fn scan_loader(options: &Options) -> Vec<AutorunEntry> {
    let mut entries = Vec::new();
    let preload = rooted(options, "/etc/ld.so.preload");
    if let Some(content) = read_to_string(&options.root, &preload) {
        for (line_number, line) in content.lines().enumerate() {
            let value = line.trim();
            if value.is_empty() || value.starts_with('#') {
                continue;
            }
            let mut entry = AutorunEntry::new(
                Category::Loader,
                value.to_string(),
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
            entry.note = Some("dynamic loader preload".to_string());
            entries.push(entry);
        }
    }

    for file in list_files(&options.root, &rooted(options, "/etc/ld.so.conf.d")) {
        let mut entry = AutorunEntry::new(
            Category::Loader,
            file.file_name()
                .map(|value| value.to_string_lossy().to_string())
                .unwrap_or_else(|| "ld config".to_string()),
            display_location(&file, &options.root),
            file.clone(),
        );
        entry.status = EntryStatus::Unknown;
        entry.timestamp = modified_timestamp(&options.root, &file);
        entry.note = Some("dynamic linker search path configuration".to_string());
        entries.push(entry);
    }

    entries
}

pub fn scan_network(options: &Options) -> Vec<AutorunEntry> {
    let mut entries = Vec::new();
    for dir in [
        "/etc/NetworkManager/dispatcher.d",
        "/etc/network/if-up.d",
        "/etc/network/if-down.d",
        "/etc/dhcp/dhclient-enter-hooks.d",
        "/etc/dhcp/dhclient-exit-hooks.d",
    ] {
        for script in list_files(&options.root, &rooted(options, dir)) {
            let mut entry = AutorunEntry::new(
                Category::Network,
                script
                    .file_name()
                    .map(|value| value.to_string_lossy().to_string())
                    .unwrap_or_else(|| "network hook".to_string()),
                display_location(&script, &options.root),
                script.clone(),
            );
            let in_image = in_root_path(&script, &options.root);
            entry.command = Some(in_image.display().to_string());
            entry.image_path = Some(in_image);
            entry.status = EntryStatus::Enabled;
            entry.timestamp = modified_timestamp(&options.root, &script);
            entries.push(entry);
        }
    }
    entries
}
