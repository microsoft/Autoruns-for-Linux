use crate::{
    cli::Options,
    model::{AutorunEntry, Category, EntryStatus},
};

use super::{
    display_location, in_root_path, list_dirs, list_files, modified_timestamp, resolve_in_root,
    rooted,
};

pub fn scan(options: &Options) -> Vec<AutorunEntry> {
    let mut files = vec![
        rooted(options, "/etc/profile"),
        rooted(options, "/etc/bash.bashrc"),
        rooted(options, "/etc/zsh/zprofile"),
        rooted(options, "/etc/zsh/zshrc"),
    ];
    files.extend(list_files(&rooted(options, "/etc/profile.d")));

    let profile_names = [
        ".profile",
        ".bash_profile",
        ".bash_login",
        ".bashrc",
        ".zprofile",
        ".zshrc",
    ];

    let mut homes = list_dirs(&rooted(options, "/home"));
    if options.root == std::path::Path::new("/") {
        if let Ok(home) = std::env::var("HOME") {
            homes.push(std::path::PathBuf::from(home));
        }
    }
    for home in homes {
        for name in profile_names {
            files.push(home.join(name));
        }
    }

    files.sort();
    files.dedup();

    let mut entries = Vec::new();
    for file in files
        .into_iter()
        .filter(|path| resolve_in_root(&options.root, path).is_file())
    {
        let mut entry = AutorunEntry::new(
            Category::Logon,
            file.file_name()
                .map(|value| value.to_string_lossy().to_string())
                .unwrap_or_else(|| "shell startup".to_string()),
            display_location(&file, &options.root),
            file.clone(),
        );
        let in_image = in_root_path(&file, &options.root);
        entry.command = Some(in_image.display().to_string());
        entry.image_path = Some(in_image);
        entry.status = EntryStatus::Enabled;
        entry.timestamp = modified_timestamp(&options.root, &file);
        entry.note = Some("shell startup file; inspect contents for commands".to_string());
        entries.push(entry);
    }
    entries
}
