use crate::{
    cli::Options,
    model::{AutorunEntry, Category, EntryStatus},
};

use super::{
    display_location, first_command_path, in_root_path, is_executable_file, list_files,
    modified_timestamp, read_to_string, rooted,
};

enum CrontabKind<'a> {
    System,
    User(&'a str),
}

pub fn scan(options: &Options) -> Vec<AutorunEntry> {
    let mut entries = Vec::new();

    let system_crontab = rooted(options, "/etc/crontab");
    if let Some(content) = read_to_string(&options.root, &system_crontab) {
        entries.extend(parse_crontab(
            options,
            &system_crontab,
            &content,
            CrontabKind::System,
        ));
    }

    for file in list_files(&options.root, &rooted(options, "/etc/cron.d")) {
        if !eligible_run_parts_path(&file) {
            continue;
        }
        if let Some(content) = read_to_string(&options.root, &file) {
            entries.extend(parse_crontab(options, &file, &content, CrontabKind::System));
        }
    }

    for spool in ["/var/spool/cron/crontabs", "/var/spool/cron"] {
        for file in list_files(&options.root, &rooted(options, spool)) {
            let Some(principal) = file.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            if principal.is_empty() || principal.contains('.') {
                continue;
            }
            if let Some(content) = read_to_string(&options.root, &file) {
                entries.extend(parse_crontab(
                    options,
                    &file,
                    &content,
                    CrontabKind::User(principal),
                ));
            }
        }
    }

    let anacrontab = rooted(options, "/etc/anacrontab");
    if let Some(content) = read_to_string(&options.root, &anacrontab) {
        entries.extend(parse_anacrontab(options, &anacrontab, &content));
    }

    for (dir_name, schedule) in [
        ("/etc/cron.hourly", "hourly"),
        ("/etc/cron.daily", "daily"),
        ("/etc/cron.weekly", "weekly"),
        ("/etc/cron.monthly", "monthly"),
    ] {
        for script in list_files(&options.root, &rooted(options, dir_name)) {
            if !eligible_run_parts_path(&script) || !is_executable_file(&options.root, &script) {
                continue;
            }
            let mut entry = AutorunEntry::new(
                Category::ScheduledTasks,
                script
                    .file_name()
                    .map(|value| value.to_string_lossy().to_string())
                    .unwrap_or_else(|| "cron script".to_string()),
                display_location(&script, &options.root),
                script.clone(),
            );
            let in_image = in_root_path(&script, &options.root);
            entry.image_path = Some(in_image.clone());
            entry.command = Some(in_image.display().to_string());
            entry.status = EntryStatus::Enabled;
            entry.timestamp = modified_timestamp(&options.root, &script);
            entry.note = Some("eligible executable in run-parts cron directory".to_string());
            entry.event = Some(schedule.to_string());
            entry.mechanism = Some("cron run-parts".to_string());
            entry.principal = Some("root".to_string());
            entry.activating_entity = Some(dir_name.to_string());
            entry.target = entry.command.clone();
            entry.completeness = Some("complete for configured run-parts directory".to_string());
            entries.push(entry);
        }
    }

    entries
}

fn parse_crontab(
    options: &Options,
    path: &std::path::Path,
    content: &str,
    kind: CrontabKind<'_>,
) -> Vec<AutorunEntry> {
    let mut entries = Vec::new();
    for (line_number, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty()
            || trimmed.starts_with('#')
            || (!trimmed.starts_with('@') && is_environment_assignment(trimmed))
        {
            continue;
        }
        let Some((schedule, principal, command)) = cron_fields(trimmed, &kind) else {
            continue;
        };
        let mut entry = AutorunEntry::new(
            Category::ScheduledTasks,
            format!("cron line {}", line_number + 1),
            format!(
                "{}:{}",
                display_location(path, &options.root),
                line_number + 1
            ),
            path.to_path_buf(),
        );
        entry.command = Some(command.clone());
        entry.image_path = first_command_path(&command);
        entry.status = EntryStatus::Enabled;
        entry.timestamp = modified_timestamp(&options.root, path);
        entry.event = Some(schedule);
        entry.mechanism = Some("cron".to_string());
        entry.principal = Some(principal);
        entry.activating_entity = Some(display_location(path, &options.root));
        entry.target = Some(command);
        entry.completeness = Some("complete for parsed crontab line".to_string());
        entries.push(entry);
    }
    entries
}

fn is_environment_assignment(line: &str) -> bool {
    // crontab(5) environment settings have the form `name = value`, where the
    // spaces around '=' are optional. The name is a single shell-style
    // identifier, so anything with whitespace before '=' is a job, not a
    // variable assignment.
    let Some((key, _)) = line.split_once('=') else {
        return false;
    };
    let key = key.trim();
    !key.is_empty()
        && !key.contains(char::is_whitespace)
        && key.starts_with(|first: char| first.is_ascii_alphabetic() || first == '_')
        && key
            .chars()
            .all(|value| value.is_ascii_alphanumeric() || value == '_')
}

fn cron_fields(line: &str, kind: &CrontabKind<'_>) -> Option<(String, String, String)> {
    let fields: Vec<&str> = line.split_whitespace().collect();
    if line.starts_with('@') {
        return match kind {
            CrontabKind::System if fields.len() >= 3 => Some((
                fields[0].to_string(),
                fields[1].to_string(),
                fields[2..].join(" "),
            )),
            CrontabKind::User(principal) if fields.len() >= 2 => Some((
                fields[0].to_string(),
                (*principal).to_string(),
                fields[1..].join(" "),
            )),
            _ => None,
        };
    }

    match kind {
        CrontabKind::System if fields.len() >= 7 => Some((
            fields[..5].join(" "),
            fields[5].to_string(),
            fields[6..].join(" "),
        )),
        CrontabKind::User(principal) if fields.len() >= 6 => Some((
            fields[..5].join(" "),
            (*principal).to_string(),
            fields[5..].join(" "),
        )),
        _ => None,
    }
}

fn parse_anacrontab(options: &Options, path: &std::path::Path, content: &str) -> Vec<AutorunEntry> {
    let mut entries = Vec::new();
    for (line_number, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') || is_environment_assignment(trimmed) {
            continue;
        }
        let fields: Vec<&str> = trimmed.split_whitespace().collect();
        if fields.len() < 4 {
            continue;
        }
        let command = fields[3..].join(" ");
        let mut entry = AutorunEntry::new(
            Category::ScheduledTasks,
            fields[2],
            format!(
                "{}:{}",
                display_location(path, &options.root),
                line_number + 1
            ),
            path.to_path_buf(),
        );
        entry.command = Some(command.clone());
        entry.image_path = first_command_path(&command);
        entry.status = EntryStatus::Enabled;
        entry.timestamp = modified_timestamp(&options.root, path);
        entry.event = Some(format!(
            "every {} days after {} minute delay",
            fields[0], fields[1]
        ));
        entry.mechanism = Some("anacron".to_string());
        entry.principal = Some("root".to_string());
        entry.activating_entity = Some("/etc/anacrontab".to_string());
        entry.target = Some(command);
        entry.completeness = Some("complete for parsed anacron job".to_string());
        entries.push(entry);
    }
    entries
}

fn eligible_run_parts_path(path: &std::path::Path) -> bool {
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
