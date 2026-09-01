use std::collections::{BTreeMap, HashMap, HashSet};

use crate::{
    cli::Options,
    model::{AutorunEntry, Category, EntryStatus},
};

use super::{
    directory_identity, display_location, first_command_path, list_dirs, list_files,
    modified_timestamp, read_link_in_root, read_to_string, rooted, user_homes,
};

#[derive(Clone)]
struct Scope {
    principal: String,
    user: bool,
    dirs: Vec<std::path::PathBuf>,
}

#[derive(Clone, Default)]
struct UnitConfig {
    values: HashMap<String, String>,
    commands: Vec<UnitCommand>,
    drop_ins: Vec<std::path::PathBuf>,
}

#[derive(Clone)]
struct UnitCommand {
    phase: String,
    command: String,
}

#[derive(Clone)]
struct UnitRecord {
    name: String,
    path: std::path::PathBuf,
    masked: bool,
    alias_target: Option<String>,
    config: UnitConfig,
}

#[derive(Default)]
struct ScopeUnits {
    effective: BTreeMap<String, UnitRecord>,
    shadowed: Vec<UnitRecord>,
    activators: HashMap<String, Vec<String>>,
}

pub fn scan(
    options: &Options,
    include_services: bool,
    include_timers: bool,
    include_devices: bool,
) -> Vec<AutorunEntry> {
    let mut entries = Vec::new();
    for scope in scopes(options) {
        let units = discover_scope(options, &scope);
        if include_services {
            emit_services(options, &scope, &units, &mut entries);
            emit_trigger_units(options, &scope, &units, "socket", &mut entries);
        }
        if include_timers {
            emit_timers(options, &scope, &units, &mut entries);
        }
        if include_devices {
            for extension in ["device", "mount", "automount", "path"] {
                emit_trigger_units(options, &scope, &units, extension, &mut entries);
            }
        }
    }
    entries
}

fn scopes(options: &Options) -> Vec<Scope> {
    let mut scopes = vec![Scope {
        principal: "system".to_string(),
        user: false,
        dirs: unique_dirs(
            options,
            [
                "/etc/systemd/system",
                "/run/systemd/system",
                "/usr/local/lib/systemd/system",
                "/usr/lib/systemd/system",
                "/lib/systemd/system",
            ]
            .into_iter()
            .map(|path| rooted(options, path))
            .collect(),
        ),
    }];

    let users = user_homes(options);
    if users.is_empty() {
        scopes.push(user_scope(options, "any user", None));
    } else {
        for user in users {
            scopes.push(user_scope(options, &user.principal, Some(user.path)));
        }
    }
    scopes
}

fn user_scope(options: &Options, principal: &str, home: Option<std::path::PathBuf>) -> Scope {
    let mut dirs = Vec::new();
    if let Some(home) = home {
        dirs.push(home.join(".config/systemd/user"));
        dirs.push(home.join(".local/share/systemd/user"));
    }
    dirs.extend(
        [
            "/etc/systemd/user",
            "/run/systemd/user",
            "/usr/local/lib/systemd/user",
            "/usr/lib/systemd/user",
            "/lib/systemd/user",
        ]
        .into_iter()
        .map(|path| rooted(options, path)),
    );
    Scope {
        principal: principal.to_string(),
        user: true,
        dirs: unique_dirs(options, dirs),
    }
}

fn unique_dirs(options: &Options, dirs: Vec<std::path::PathBuf>) -> Vec<std::path::PathBuf> {
    let mut seen = HashSet::new();
    dirs.into_iter()
        .filter(|dir| {
            directory_identity(&options.root, dir)
                .map(|identity| seen.insert(identity))
                .unwrap_or(true)
        })
        .collect()
}

fn discover_scope(options: &Options, scope: &Scope) -> ScopeUnits {
    let mut units = ScopeUnits::default();
    for dir in &scope.dirs {
        for path in list_files(&options.root, dir) {
            let Some(name) = path
                .file_name()
                .and_then(|value| value.to_str())
                .map(str::to_string)
            else {
                continue;
            };
            if !is_supported_unit(&name) {
                continue;
            }
            let record = load_record(options, scope, &name, path);
            match units.effective.entry(name) {
                std::collections::btree_map::Entry::Vacant(entry) => {
                    entry.insert(record);
                }
                std::collections::btree_map::Entry::Occupied(_) => {
                    units.shadowed.push(record);
                }
            }
        }
    }

    units.activators = collect_activators(options, scope);
    let activated_names: Vec<String> = units.activators.keys().cloned().collect();
    for name in activated_names {
        if units.effective.contains_key(&name) {
            continue;
        }
        let Some(template) = template_name(&name) else {
            continue;
        };
        if let Some(record) = units.effective.get(&template).cloned() {
            let mut instance = record;
            instance.name = name.clone();
            units.effective.insert(name, instance);
        }
    }
    units
}

fn load_record(
    options: &Options,
    scope: &Scope,
    name: &str,
    path: std::path::PathBuf,
) -> UnitRecord {
    let masked = is_dev_null_mask(&options.root, &path);
    let alias_target = link_target_name(&options.root, &path).filter(|target| target != "null");
    let config = if masked {
        UnitConfig::default()
    } else {
        load_config(options, scope, name, &path)
    };
    UnitRecord {
        name: name.to_string(),
        path,
        masked,
        alias_target,
        config,
    }
}

fn load_config(options: &Options, scope: &Scope, name: &str, path: &std::path::Path) -> UnitConfig {
    let mut config = UnitConfig::default();
    if let Some(content) = read_to_string(&options.root, path) {
        parse_unit_content(&content, &mut config);
    }

    let mut selected = BTreeMap::<String, std::path::PathBuf>::new();
    for dir in scope.dirs.iter().rev() {
        for drop_in_dir in drop_in_dirs(name) {
            for file in list_files(&options.root, &dir.join(drop_in_dir)) {
                if file.extension().and_then(|value| value.to_str()) == Some("conf") {
                    if let Some(file_name) = file.file_name().and_then(|value| value.to_str()) {
                        selected.insert(file_name.to_string(), file);
                    }
                }
            }
        }
    }
    for path in selected.into_values() {
        if let Some(content) = read_to_string(&options.root, &path) {
            parse_unit_content(&content, &mut config);
            config.drop_ins.push(path);
        }
    }
    config
}

fn drop_in_dirs(name: &str) -> Vec<String> {
    let Some((stem, extension)) = name.rsplit_once('.') else {
        return vec![format!("{name}.d")];
    };
    let mut dirs = vec![format!("{extension}.d")];
    let mut prefix = stem;
    let mut prefixes = Vec::new();
    while let Some(index) = prefix.rfind('-') {
        prefix = &prefix[..index];
        prefixes.push(format!("{prefix}-.{extension}.d"));
    }
    prefixes.reverse();
    dirs.extend(prefixes);
    dirs.push(format!("{name}.d"));
    dirs
}

fn parse_unit_content(content: &str, config: &mut UnitConfig) {
    let mut section = String::new();
    for line in logical_lines(content) {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        if let Some(name) = line
            .strip_prefix('[')
            .and_then(|line| line.strip_suffix(']'))
        {
            name.trim().clone_into(&mut section);
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        let value = value.trim();
        if section == "Service" && is_startup_command_key(key) {
            if value.is_empty() {
                config.commands.retain(|command| command.phase != key);
            } else {
                config.commands.push(UnitCommand {
                    phase: key.to_string(),
                    command: value.to_string(),
                });
            }
        } else if is_multi_value_key(key) && !value.is_empty() {
            config
                .values
                .entry(key.to_string())
                .and_modify(|existing| {
                    existing.push_str("; ");
                    existing.push_str(value);
                })
                .or_insert_with(|| value.to_string());
        } else {
            config.values.insert(key.to_string(), value.to_string());
        }
    }
}

fn is_startup_command_key(key: &str) -> bool {
    matches!(
        key,
        "ExecCondition" | "ExecStartPre" | "ExecStart" | "ExecStartPost"
    )
}

fn logical_lines(content: &str) -> Vec<String> {
    let mut lines = Vec::new();
    let mut current = String::new();
    for physical in content.lines() {
        let trimmed = physical.trim_end();
        let slash_count = trimmed
            .chars()
            .rev()
            .take_while(|value| *value == '\\')
            .count();
        let continued = slash_count % 2 == 1;
        let part = if continued {
            &trimmed[..trimmed.len() - 1]
        } else {
            trimmed
        };
        if !current.is_empty() {
            current.push(' ');
        }
        current.push_str(part.trim());
        if !continued {
            lines.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        lines.push(current);
    }
    lines
}

fn is_multi_value_key(key: &str) -> bool {
    matches!(
        key,
        "OnCalendar"
            | "OnBootSec"
            | "OnStartupSec"
            | "OnUnitActiveSec"
            | "OnUnitInactiveSec"
            | "PathExists"
            | "PathExistsGlob"
            | "PathChanged"
            | "PathModified"
            | "DirectoryNotEmpty"
            | "ListenStream"
            | "ListenDatagram"
            | "ListenSequentialPacket"
            | "ListenFIFO"
            | "ListenSpecial"
            | "WantedBy"
            | "RequiredBy"
            | "Wants"
            | "Requires"
    )
}

fn collect_activators(options: &Options, scope: &Scope) -> HashMap<String, Vec<String>> {
    let mut activators: HashMap<String, Vec<String>> = HashMap::new();
    for base in &scope.dirs {
        for dir in list_dirs(&options.root, base) {
            let relation = dir
                .extension()
                .and_then(|value| value.to_str())
                .filter(|value| matches!(*value, "wants" | "requires"));
            if relation.is_none() {
                continue;
            }
            for path in list_files(&options.root, &dir) {
                if !is_symlink(&options.root, &path) {
                    continue;
                }
                let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
                    continue;
                };
                activators
                    .entry(name.to_string())
                    .or_default()
                    .push(display_location(&dir, &options.root));
            }
        }
    }
    for values in activators.values_mut() {
        values.sort();
        values.dedup();
    }
    activators
}

fn emit_services(
    options: &Options,
    scope: &Scope,
    units: &ScopeUnits,
    entries: &mut Vec<AutorunEntry>,
) {
    for record in units
        .effective
        .values()
        .filter(|record| extension(&record.name) == Some("service"))
    {
        emit_service_record(options, scope, units, record, entries);
    }
    for record in units
        .shadowed
        .iter()
        .filter(|record| extension(&record.name) == Some("service"))
    {
        entries.push(shadowed_entry(options, scope, record, Category::Services));
    }
}

fn emit_service_record(
    options: &Options,
    scope: &Scope,
    units: &ScopeUnits,
    record: &UnitRecord,
    entries: &mut Vec<AutorunEntry>,
) {
    let status = unit_status(record, units);
    if record.masked || record.config.commands.is_empty() {
        entries.push(base_entry(
            options,
            scope,
            units,
            record,
            Category::Services,
            status,
        ));
        return;
    }
    for command in &record.config.commands {
        let expanded = expand_specifiers(&command.command, &record.name);
        let mut entry = base_entry(options, scope, units, record, Category::Services, status);
        entry.command = Some(expanded.clone());
        entry.image_path = first_command_path(&expanded);
        entry.target = Some(expanded);
        entry.mechanism = Some(format!("systemd {}", command.phase));
        entries.push(entry);
    }
}

fn emit_timers(
    options: &Options,
    scope: &Scope,
    units: &ScopeUnits,
    entries: &mut Vec<AutorunEntry>,
) {
    for timer in units
        .effective
        .values()
        .filter(|record| extension(&record.name) == Some("timer"))
    {
        let target = timer
            .config
            .values
            .get("Unit")
            .cloned()
            .unwrap_or_else(|| replace_extension(&timer.name, "service"));
        let event = joined_values(
            &timer.config,
            &[
                "OnCalendar",
                "OnBootSec",
                "OnStartupSec",
                "OnUnitActiveSec",
                "OnUnitInactiveSec",
            ],
        );
        let status = unit_status(timer, units);
        let payload = units.effective.get(&target);
        if let Some(payload) = payload.filter(|payload| !payload.config.commands.is_empty()) {
            for command in &payload.config.commands {
                let expanded = expand_specifiers(&command.command, &payload.name);
                let mut entry = base_entry(
                    options,
                    scope,
                    units,
                    timer,
                    Category::ScheduledTasks,
                    status,
                );
                entry.event = event
                    .clone()
                    .or_else(|| Some("systemd timer expiry".to_string()));
                entry.mechanism = Some(format!("systemd timer -> {}", command.phase));
                entry.activating_entity = Some(timer.name.clone());
                entry.target = Some(target.clone());
                entry.command = Some(expanded.clone());
                entry.image_path = first_command_path(&expanded);
                entries.push(entry);
            }
        } else {
            let mut entry = base_entry(
                options,
                scope,
                units,
                timer,
                Category::ScheduledTasks,
                status,
            );
            entry.event = event.or_else(|| Some("systemd timer expiry".to_string()));
            entry.mechanism = Some("systemd timer".to_string());
            entry.activating_entity = Some(timer.name.clone());
            entry.target = Some(target);
            entry.completeness = Some("target unit payload was not found".to_string());
            entries.push(entry);
        }
    }
    for record in units
        .shadowed
        .iter()
        .filter(|record| extension(&record.name) == Some("timer"))
    {
        entries.push(shadowed_entry(
            options,
            scope,
            record,
            Category::ScheduledTasks,
        ));
    }
}

fn emit_trigger_units(
    options: &Options,
    scope: &Scope,
    units: &ScopeUnits,
    unit_extension: &str,
    entries: &mut Vec<AutorunEntry>,
) {
    let category = if unit_extension == "socket" {
        Category::Services
    } else {
        Category::DeviceMount
    };
    for record in units
        .effective
        .values()
        .filter(|record| extension(&record.name) == Some(unit_extension))
    {
        let target = record
            .config
            .values
            .get("Unit")
            .cloned()
            .unwrap_or_else(|| replace_extension(&record.name, "service"));
        let trigger_keys: &[&str] = match unit_extension {
            "path" => &[
                "PathExists",
                "PathExistsGlob",
                "PathChanged",
                "PathModified",
                "DirectoryNotEmpty",
            ],
            "socket" => &[
                "ListenStream",
                "ListenDatagram",
                "ListenSequentialPacket",
                "ListenFIFO",
                "ListenSpecial",
            ],
            "mount" => &["What", "Where"],
            "automount" => &["Where"],
            _ => &[],
        };
        let event = joined_values(&record.config, trigger_keys)
            .or_else(|| Some(format!("systemd {unit_extension} activation")));
        if let Some(payload) = units
            .effective
            .get(&target)
            .filter(|payload| !payload.config.commands.is_empty())
        {
            for command in &payload.config.commands {
                let expanded = expand_specifiers(&command.command, &payload.name);
                let mut entry = base_entry(
                    options,
                    scope,
                    units,
                    record,
                    category,
                    unit_status(record, units),
                );
                entry.event = event.clone();
                entry.mechanism = Some(format!("systemd {unit_extension} -> {}", command.phase));
                entry.activating_entity = Some(record.name.clone());
                entry.target = Some(target.clone());
                entry.command = Some(expanded.clone());
                entry.image_path = first_command_path(&expanded);
                entries.push(entry);
            }
        } else {
            let mut entry = base_entry(
                options,
                scope,
                units,
                record,
                category,
                unit_status(record, units),
            );
            entry.event = event;
            entry.mechanism = Some(format!("systemd {unit_extension} unit"));
            entry.activating_entity = Some(record.name.clone());
            entry.target = Some(target);
            entries.push(entry);
        }
    }
}

fn base_entry(
    options: &Options,
    scope: &Scope,
    units: &ScopeUnits,
    record: &UnitRecord,
    category: Category,
    status: EntryStatus,
) -> AutorunEntry {
    let mut entry = AutorunEntry::new(
        category,
        &record.name,
        display_location(&record.path, &options.root),
        record.path.clone(),
    );
    entry.description = record.config.values.get("Description").cloned();
    entry.status = status;
    if !record.masked {
        entry.timestamp = modified_timestamp(&options.root, &record.path);
    }
    entry.event = Some(
        if scope.user {
            "user manager activation"
        } else {
            "system manager activation"
        }
        .to_string(),
    );
    entry.mechanism = Some("systemd unit".to_string());
    entry.principal = Some(scope.principal.clone());
    entry.activating_entity = units
        .activators
        .get(&record.name)
        .map(|values| values.join("; "));
    entry.completeness =
        Some("effective unit, drop-ins, scope, and activation links evaluated".to_string());

    let mut notes = Vec::new();
    if record.masked {
        notes.push("masked by /dev/null".to_string());
    }
    if let Some(alias) = &record.alias_target {
        notes.push(format!("alias target={alias}"));
    }
    if !record.config.drop_ins.is_empty() {
        notes.push(format!("drop-ins={}", record.config.drop_ins.len()));
    }
    if let Some(wanted_by) = record.config.values.get("WantedBy") {
        notes.push(format!("WantedBy={wanted_by}"));
    }
    if !notes.is_empty() {
        entry.note = Some(notes.join("; "));
    }
    entry
}

fn shadowed_entry(
    options: &Options,
    scope: &Scope,
    record: &UnitRecord,
    category: Category,
) -> AutorunEntry {
    let mut entry = AutorunEntry::new(
        category,
        &record.name,
        display_location(&record.path, &options.root),
        record.path.clone(),
    );
    entry.status = EntryStatus::Shadowed;
    entry.principal = Some(scope.principal.clone());
    entry.event = Some("systemd configuration load".to_string());
    entry.mechanism = Some("lower-priority systemd unit".to_string());
    entry.note = Some("shadowed by a higher-priority same-name unit".to_string());
    entry.completeness = Some("retained as non-effective evidence".to_string());
    entry
}

fn unit_status(record: &UnitRecord, units: &ScopeUnits) -> EntryStatus {
    if record.masked {
        EntryStatus::Disabled
    } else if units.activators.contains_key(&record.name) {
        EntryStatus::Enabled
    } else {
        EntryStatus::Unknown
    }
}

fn joined_values(config: &UnitConfig, keys: &[&str]) -> Option<String> {
    let values: Vec<String> = keys
        .iter()
        .filter_map(|key| {
            config
                .values
                .get(*key)
                .map(|value| format!("{key}={value}"))
        })
        .collect();
    (!values.is_empty()).then(|| values.join("; "))
}

fn expand_specifiers(command: &str, name: &str) -> String {
    let stem = name.rsplit_once('.').map(|value| value.0).unwrap_or(name);
    let instance = stem.split_once('@').map(|value| value.1).unwrap_or("");
    command
        .replace("%%", "\u{0000}")
        .replace("%i", instance)
        .replace("%I", instance)
        .replace("%n", name)
        .replace("%N", stem)
        .replace('\u{0000}', "%")
}

fn replace_extension(name: &str, extension: &str) -> String {
    name.rsplit_once('.')
        .map(|(stem, _)| format!("{stem}.{extension}"))
        .unwrap_or_else(|| format!("{name}.{extension}"))
}

fn template_name(name: &str) -> Option<String> {
    let dot = name.rfind('.')?;
    let at = name[..dot].find('@')?;
    if at + 1 == dot {
        return None;
    }
    Some(format!("{}@{}", &name[..at], &name[dot..]))
}

fn extension(name: &str) -> Option<&str> {
    name.rsplit_once('.').map(|value| value.1)
}

fn is_supported_unit(name: &str) -> bool {
    matches!(
        extension(name),
        Some("service" | "timer" | "socket" | "path" | "device" | "mount" | "automount")
    )
}

fn is_dev_null_mask(root: &std::path::Path, path: &std::path::Path) -> bool {
    link_target(root, path)
        .map(|target| target == std::path::Path::new("/dev/null"))
        .unwrap_or(false)
}

fn link_target_name(root: &std::path::Path, path: &std::path::Path) -> Option<String> {
    link_target(root, path).and_then(|target| {
        target
            .file_name()
            .map(|value| value.to_string_lossy().to_string())
    })
}

fn link_target(root: &std::path::Path, path: &std::path::Path) -> Option<std::path::PathBuf> {
    read_link_in_root(root, path).ok()
}

fn is_symlink(root: &std::path::Path, path: &std::path::Path) -> bool {
    read_link_in_root(root, path).is_ok()
}
