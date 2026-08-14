use crate::{
    cli::Options,
    model::{AutorunEntry, Category, EntryStatus},
};

use super::{display_location, list_dirs, list_files, modified_timestamp, rooted};

pub fn scan(options: &Options) -> Vec<AutorunEntry> {
    let mut files = vec![
        rooted(options, "/etc/profile"),
        rooted(options, "/etc/bash.bashrc"),
        rooted(options, "/etc/zsh/zprofile"),
        rooted(options, "/etc/zsh/zshrc"),
    ];
    files.extend(list_files(&rooted(options, "/etc/profile.d")));

    for home in list_dirs(&rooted(options, "/home")) {
        for name in [
            ".profile",
            ".bash_profile",
            ".bash_login",
            ".bashrc",
            ".zprofile",
            ".zshrc",
        ] {
            files.push(home.join(name));
        }
    }

    let mut entries = Vec::new();
    for file in files.into_iter().filter(|path| path.is_file()) {
        let mut entry = AutorunEntry::new(
            Category::Logon,
            file.file_name()
                .map(|value| value.to_string_lossy().to_string())
                .unwrap_or_else(|| "shell startup".to_string()),
            display_location(&file, &options.root),
            file.clone(),
        );
        entry.command = Some(file.display().to_string());
        entry.image_path = Some(file.clone());
        entry.status = EntryStatus::Enabled;
        entry.timestamp = modified_timestamp(&file);
        entry.note = Some("shell startup file; inspect contents for commands".to_string());
        entries.push(entry);
    }
    entries
}
