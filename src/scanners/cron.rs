use crate::{
    cli::Options,
    model::{AutorunEntry, Category, EntryStatus},
};

use super::{
    display_location, first_command_path, in_root_path, list_files, modified_timestamp,
    read_to_string, rooted,
};

pub fn scan(options: &Options) -> Vec<AutorunEntry> {
    let mut entries = Vec::new();

    for file in [rooted(options, "/etc/crontab")]
        .into_iter()
        .chain(list_files(&options.root, &rooted(options, "/etc/cron.d")))
    {
        if let Some(content) = read_to_string(&options.root, &file) {
            entries.extend(parse_crontab(options, &file, &content));
        }
    }

    for dir_name in [
        "/etc/cron.hourly",
        "/etc/cron.daily",
        "/etc/cron.weekly",
        "/etc/cron.monthly",
    ] {
        for script in list_files(&options.root, &rooted(options, dir_name)) {
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
        if trimmed.is_empty()
            || trimmed.starts_with('#')
            || (!trimmed.starts_with('@') && is_environment_assignment(trimmed))
        {
            continue;
        }
        let Some(command) = cron_command(trimmed) else {
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

fn cron_command(line: &str) -> Option<String> {
    if line.starts_with('@') {
        // System crontab entries (/etc/crontab, /etc/cron.d) place a user
        // field between the schedule macro and the command.
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() < 3 {
            return None;
        }
        return Some(fields[2..].join(" "));
    }

    // Non-macro entries in /etc/crontab and /etc/cron.d are system crontab
    // format: five schedule fields, then a mandatory user field, then the
    // command. Require that user field rather than guessing whether it is
    // present, so a command token is never mistaken for the user (or dropped).
    let fields: Vec<&str> = line.split_whitespace().collect();
    if fields.len() < 7 {
        return None;
    }

    Some(
        line.split_whitespace()
            .skip(6)
            .collect::<Vec<_>>()
            .join(" "),
    )
}
