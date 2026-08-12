use crate::{cli::Options, model::{AutorunEntry, Category, EntryStatus}};

use super::{display_location, first_command_path, list_files, modified_timestamp, read_to_string, rooted};

pub fn scan(options: &Options) -> Vec<AutorunEntry> {
    let mut entries = Vec::new();

    for file in [rooted(options, "/etc/crontab")]
        .into_iter()
        .chain(list_files(&rooted(options, "/etc/cron.d")))
    {
        if let Some(content) = read_to_string(&file) {
            entries.extend(parse_crontab(options, &file, &content));
        }
    }

    for dir_name in ["/etc/cron.hourly", "/etc/cron.daily", "/etc/cron.weekly", "/etc/cron.monthly"] {
        for script in list_files(&rooted(options, dir_name)) {
            let mut entry = AutorunEntry::new(
                Category::ScheduledTasks,
                script.file_name().map(|value| value.to_string_lossy().to_string()).unwrap_or_else(|| "cron script".to_string()),
                display_location(&script, &options.root),
                script.clone(),
            );
            entry.image_path = Some(script.clone());
            entry.command = Some(script.display().to_string());
            entry.status = EntryStatus::Enabled;
            entry.timestamp = modified_timestamp(&script);
            entry.note = Some("run-parts cron directory".to_string());
            entries.push(entry);
        }
    }

    entries
}

fn parse_crontab(options: &Options, path: &std::path::Path, content: &str) -> Vec<AutorunEntry> {
    let mut entries = Vec::new();
    for (line_number, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.contains('=') && !trimmed.starts_with('@') {
            continue;
        }
        let Some(command) = cron_command(trimmed) else {
            continue;
        };
        let mut entry = AutorunEntry::new(
            Category::ScheduledTasks,
            format!("cron line {}", line_number + 1),
            format!("{}:{}", display_location(path, &options.root), line_number + 1),
            path.to_path_buf(),
        );
        entry.command = Some(command.clone());
        entry.image_path = first_command_path(&command);
        entry.status = EntryStatus::Enabled;
        entry.timestamp = modified_timestamp(path);
        entries.push(entry);
    }
    entries
}

fn cron_command(line: &str) -> Option<String> {
    if line.starts_with('@') {
        return line
            .split_once(char::is_whitespace)
            .map(|(_, command)| command.trim())
            .filter(|value| !value.is_empty())
            .map(str::to_string);
    }

    let fields: Vec<&str> = line.split_whitespace().collect();
    if fields.len() < 6 {
        return None;
    }

    let command_index = if fields.len() >= 7 && !fields[5].contains('/') && !fields[5].contains('*') {
        6
    } else {
        5
    };

    Some(line.split_whitespace().skip(command_index).collect::<Vec<_>>().join(" "))
}