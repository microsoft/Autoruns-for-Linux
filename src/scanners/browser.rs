use std::collections::{HashMap, HashSet};
use std::io::Read;

use serde_json::Value;

use crate::{
    cli::Options,
    model::{AutorunEntry, Category, EntryStatus},
};

use super::{
    display_location, first_command_path, in_root_path, list_dirs, list_files, modified_timestamp,
    open_file_in_root, path_is_dir, path_is_file, read_to_string, record_diagnostic, rooted,
    shell_tokens, user_homes,
};

#[derive(Clone, Default)]
struct ExtensionMeta {
    name: String,
    version: String,
    manifest_version: String,
}

pub fn scan(options: &Options) -> Vec<AutorunEntry> {
    let mut entries = Vec::new();
    let mut manifest_cache = HashMap::new();

    for user in user_homes(options) {
        for (browser, root) in chromium_roots(&user.path) {
            scan_chromium_root(
                options,
                browser,
                &user.principal,
                &root,
                &mut manifest_cache,
                &mut entries,
            );
        }
        for root in firefox_roots(&user.path) {
            scan_firefox_root(
                options,
                &user.principal,
                &root,
                &mut manifest_cache,
                &mut entries,
            );
        }
        scan_launchers(
            options,
            &user.principal,
            &user.path.join(".local/share/applications"),
            &mut entries,
        );
    }

    for (browser, policy_root) in chromium_policy_roots(options) {
        scan_policy_root(options, browser, &policy_root, &mut entries);
    }
    scan_policy_root(
        options,
        "Firefox",
        &rooted(options, "/etc/firefox/policies"),
        &mut entries,
    );
    scan_json_policy_file(
        options,
        "Firefox",
        &rooted(options, "/usr/lib/firefox/distribution/policies.json"),
        &mut entries,
    );

    for (browser, path) in system_native_host_dirs(options) {
        scan_native_host_dir(options, browser, "all users", None, &path, &mut entries);
    }
    for (browser, path) in external_extension_dirs(options) {
        scan_external_extensions(options, browser, &path, &mut entries);
    }
    for path in [
        rooted(options, "/usr/share/applications"),
        rooted(options, "/usr/local/share/applications"),
    ] {
        scan_launchers(options, "all users", &path, &mut entries);
    }

    entries
}

fn chromium_roots(home: &std::path::Path) -> Vec<(&'static str, std::path::PathBuf)> {
    [
        ("Google Chrome", ".config/google-chrome"),
        ("Google Chrome Beta", ".config/google-chrome-beta"),
        ("Google Chrome Unstable", ".config/google-chrome-unstable"),
        ("Chromium", ".config/chromium"),
        ("Microsoft Edge", ".config/microsoft-edge"),
        ("Microsoft Edge Beta", ".config/microsoft-edge-beta"),
        ("Microsoft Edge Dev", ".config/microsoft-edge-dev"),
        ("Brave", ".config/BraveSoftware/Brave-Browser"),
        ("Brave Beta", ".config/BraveSoftware/Brave-Browser-Beta"),
        (
            "Brave Nightly",
            ".config/BraveSoftware/Brave-Browser-Nightly",
        ),
        ("Vivaldi", ".config/vivaldi"),
        ("Vivaldi Snapshot", ".config/vivaldi-snapshot"),
        ("Opera", ".config/opera"),
        ("Chromium (Snap)", "snap/chromium/common/chromium"),
        (
            "Google Chrome (Flatpak)",
            ".var/app/com.google.Chrome/config/google-chrome",
        ),
        (
            "Chromium (Flatpak)",
            ".var/app/org.chromium.Chromium/config/chromium",
        ),
        (
            "Microsoft Edge (Flatpak)",
            ".var/app/com.microsoft.Edge/config/microsoft-edge",
        ),
        (
            "Brave (Flatpak)",
            ".var/app/com.brave.Browser/config/BraveSoftware/Brave-Browser",
        ),
    ]
    .into_iter()
    .map(|(browser, path)| (browser, home.join(path)))
    .collect()
}

fn firefox_roots(home: &std::path::Path) -> Vec<std::path::PathBuf> {
    [
        ".mozilla/firefox",
        "snap/firefox/common/.mozilla/firefox",
        ".var/app/org.mozilla.firefox/.mozilla/firefox",
    ]
    .into_iter()
    .map(|path| home.join(path))
    .collect()
}

fn scan_chromium_root(
    options: &Options,
    browser: &str,
    principal: &str,
    root: &std::path::Path,
    cache: &mut HashMap<std::path::PathBuf, ExtensionMeta>,
    entries: &mut Vec<AutorunEntry>,
) {
    if !is_dir(options, root) {
        return;
    }
    let mut profiles: Vec<_> = list_dirs(&options.root, root)
        .into_iter()
        .filter(|path| {
            let name = path
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or("");
            name == "Default"
                || name.starts_with("Profile ")
                || path_exists(options, &path.join("Preferences"))
                || is_dir(options, &path.join("Extensions"))
        })
        .collect();
    if path_exists(options, &root.join("Preferences")) || is_dir(options, &root.join("Extensions"))
    {
        profiles.push(root.to_path_buf());
    }
    profiles.sort();
    profiles.dedup();

    for profile in profiles {
        scan_chromium_profile(options, browser, principal, &profile, cache, entries);
    }
    scan_native_host_dir(
        options,
        browser,
        principal,
        None,
        &root.join("NativeMessagingHosts"),
        entries,
    );
}

fn scan_chromium_profile(
    options: &Options,
    browser: &str,
    principal: &str,
    profile: &std::path::Path,
    cache: &mut HashMap<std::path::PathBuf, ExtensionMeta>,
    entries: &mut Vec<AutorunEntry>,
) {
    let profile_name = profile
        .file_name()
        .map(|value| value.to_string_lossy().to_string())
        .unwrap_or_else(|| "Default".to_string());
    let preferences_path = profile.join("Preferences");
    let preferences = read_json(options, &preferences_path);
    let settings = preferences
        .as_ref()
        .and_then(|value| value.pointer("/extensions/settings"))
        .and_then(Value::as_object);
    let mut emitted = HashSet::new();

    for id_dir in list_dirs(&options.root, &profile.join("Extensions")) {
        let Some(id) = id_dir.file_name().and_then(|value| value.to_str()) else {
            continue;
        };
        let mut versions = list_dirs(&options.root, &id_dir);
        versions.sort();
        let Some(version_dir) = versions.last() else {
            continue;
        };
        let manifest = version_dir.join("manifest.json");
        let meta = manifest_meta(options, &manifest, cache);
        let setting = settings.and_then(|settings| settings.get(id));
        entries.push(browser_extension_entry(
            options,
            browser,
            principal,
            &profile_name,
            id,
            &manifest,
            &in_root_path(version_dir, &options.root),
            meta,
            chromium_status(setting),
            "Chromium extension manifest",
        ));
        emitted.insert(id.to_string());
    }

    if let Some(settings) = settings {
        for (id, setting) in settings {
            if emitted.contains(id) {
                continue;
            }
            let Some(path) = setting.get("path").and_then(Value::as_str) else {
                continue;
            };
            let path = if std::path::Path::new(path).is_absolute() {
                rooted(options, path)
            } else {
                profile.join(path)
            };
            let manifest = path.join("manifest.json");
            let meta = setting
                .get("manifest")
                .map(meta_from_json)
                .unwrap_or_else(|| manifest_meta(options, &manifest, cache));
            entries.push(browser_extension_entry(
                options,
                browser,
                principal,
                &profile_name,
                id,
                &preferences_path,
                &in_root_path(&path, &options.root),
                meta,
                chromium_status(Some(setting)),
                "Chromium unpacked/external extension registration",
            ));
        }
    }

    scan_native_host_dir(
        options,
        browser,
        principal,
        Some(&profile_name),
        &profile.join("NativeMessagingHosts"),
        entries,
    );
}

fn chromium_status(setting: Option<&Value>) -> EntryStatus {
    match setting
        .and_then(|value| value.get("state"))
        .and_then(Value::as_i64)
    {
        Some(1) => EntryStatus::Enabled,
        Some(0) => EntryStatus::Disabled,
        _ => EntryStatus::Unknown,
    }
}

#[allow(clippy::too_many_arguments)]
fn browser_extension_entry(
    options: &Options,
    browser: &str,
    principal: &str,
    profile: &str,
    id: &str,
    source: &std::path::Path,
    target: &std::path::Path,
    meta: ExtensionMeta,
    status: EntryStatus,
    mechanism: &str,
) -> AutorunEntry {
    let mut entry = AutorunEntry::new(
        Category::Browser,
        if meta.name.is_empty() {
            id.to_string()
        } else {
            meta.name.clone()
        },
        format!("{browser}/{profile}/{id}"),
        source.to_path_buf(),
    );
    entry.description = (!meta.version.is_empty()).then(|| format!("version {}", meta.version));
    entry.status = status;
    entry.timestamp = modified_timestamp(&options.root, source);
    entry.event = Some("browser profile extension load".to_string());
    entry.mechanism = Some(mechanism.to_string());
    entry.principal = Some(principal.to_string());
    entry.profile = Some(format!("{browser}/{profile}"));
    entry.activating_entity = Some(browser.to_string());
    entry.target = Some(target.display().to_string());
    if !meta.manifest_version.is_empty() {
        entry.note = Some(format!(
            "id={id}; manifest_version={}",
            meta.manifest_version
        ));
    } else {
        entry.note = Some(format!("id={id}"));
    }
    entry.completeness = Some("profile relationship preserved".to_string());
    entry
}

fn scan_firefox_root(
    options: &Options,
    principal: &str,
    root: &std::path::Path,
    cache: &mut HashMap<std::path::PathBuf, ExtensionMeta>,
    entries: &mut Vec<AutorunEntry>,
) {
    if !is_dir(options, root) {
        return;
    }
    let mut profiles = list_dirs(&options.root, root);
    profiles.extend(firefox_ini_profiles(options, root));
    profiles.sort();
    profiles.dedup();
    for profile in profiles {
        if !path_exists(options, &profile.join("extensions.json"))
            && !is_dir(options, &profile.join("extensions"))
            && !path_exists(options, &profile.join("pkcs11.txt"))
        {
            continue;
        }
        scan_firefox_profile(options, principal, &profile, cache, entries);
    }
    scan_native_host_dir(
        options,
        "Firefox",
        principal,
        None,
        &root.join("native-messaging-hosts"),
        entries,
    );
}

fn firefox_ini_profiles(options: &Options, root: &std::path::Path) -> Vec<std::path::PathBuf> {
    let Some(content) = read_to_string(&options.root, &root.join("profiles.ini")) else {
        return Vec::new();
    };
    let mut paths = Vec::new();
    let mut relative = true;
    for line in content.lines().map(str::trim) {
        if line.starts_with('[') {
            relative = true;
        } else if let Some(value) = line.strip_prefix("IsRelative=") {
            relative = value != "0";
        } else if let Some(value) = line.strip_prefix("Path=") {
            paths.push(if relative {
                root.join(value)
            } else {
                rooted(options, value)
            });
        }
    }
    paths
}

fn scan_firefox_profile(
    options: &Options,
    principal: &str,
    profile: &std::path::Path,
    cache: &mut HashMap<std::path::PathBuf, ExtensionMeta>,
    entries: &mut Vec<AutorunEntry>,
) {
    let profile_name = profile
        .file_name()
        .map(|value| value.to_string_lossy().to_string())
        .unwrap_or_else(|| "profile".to_string());
    let extensions_json = profile.join("extensions.json");
    let mut emitted = HashSet::new();
    if let Some(value) = read_json(options, &extensions_json) {
        if let Some(addons) = value.get("addons").and_then(Value::as_array) {
            for addon in addons {
                if addon.get("type").and_then(Value::as_str) != Some("extension") {
                    continue;
                }
                let id = addon.get("id").and_then(Value::as_str).unwrap_or("unknown");
                let meta = ExtensionMeta {
                    name: addon
                        .pointer("/defaultLocale/name")
                        .and_then(Value::as_str)
                        .unwrap_or(id)
                        .to_string(),
                    version: addon
                        .get("version")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string(),
                    manifest_version: String::new(),
                };
                let active = addon
                    .get("active")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
                    && !addon
                        .get("userDisabled")
                        .and_then(Value::as_bool)
                        .unwrap_or(false)
                    && !addon
                        .get("appDisabled")
                        .and_then(Value::as_bool)
                        .unwrap_or(false);
                let target = addon
                    .get("path")
                    .and_then(Value::as_str)
                    .map(std::path::PathBuf::from)
                    .unwrap_or_else(|| std::path::PathBuf::from(id));
                entries.push(browser_extension_entry(
                    options,
                    "Firefox",
                    principal,
                    &profile_name,
                    id,
                    &extensions_json,
                    &target,
                    meta,
                    if active {
                        EntryStatus::Enabled
                    } else {
                        EntryStatus::Disabled
                    },
                    "Firefox WebExtension registry",
                ));
                emitted.insert(id.to_string());
            }
        }
    }

    for extension in list_files(&options.root, &profile.join("extensions")) {
        let Some(id) = extension.file_stem().and_then(|value| value.to_str()) else {
            continue;
        };
        if emitted.contains(id) {
            continue;
        }
        let meta = xpi_meta(options, &extension, cache);
        entries.push(browser_extension_entry(
            options,
            "Firefox",
            principal,
            &profile_name,
            id,
            &extension,
            &in_root_path(&extension, &options.root),
            meta,
            EntryStatus::Unknown,
            "Firefox packed extension",
        ));
    }
    scan_pkcs11(
        options,
        principal,
        &profile_name,
        &profile.join("pkcs11.txt"),
        entries,
    );
}

fn scan_native_host_dir(
    options: &Options,
    browser: &str,
    principal: &str,
    profile: Option<&str>,
    dir: &std::path::Path,
    entries: &mut Vec<AutorunEntry>,
) {
    for path in list_files(&options.root, dir) {
        if path.extension().and_then(|value| value.to_str()) != Some("json") {
            continue;
        }
        let Some(value) = read_json(options, &path) else {
            continue;
        };
        let name = value
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("native messaging host");
        let command = value
            .get("path")
            .and_then(Value::as_str)
            .map(str::to_string);
        let mut entry = AutorunEntry::new(
            Category::Browser,
            name,
            display_location(&path, &options.root),
            path.clone(),
        );
        entry.description = value
            .get("description")
            .and_then(Value::as_str)
            .map(str::to_string);
        entry.command = command.clone();
        entry.image_path = command.as_deref().and_then(first_command_path);
        entry.status = EntryStatus::Conditional;
        entry.timestamp = modified_timestamp(&options.root, &path);
        entry.event = Some("browser extension native-messaging request".to_string());
        entry.mechanism = Some("native messaging host manifest".to_string());
        entry.principal = Some(principal.to_string());
        entry.profile = Some(format!("{browser}/{}", profile.unwrap_or("all profiles")));
        entry.activating_entity = Some(browser.to_string());
        entry.target = command;
        entry.note = Some(
            "callable by an allowed extension; not assumed to start with the browser".to_string(),
        );
        entry.completeness = Some("manifest and executable relationship parsed".to_string());
        entries.push(entry);
    }
}

fn scan_policy_root(
    options: &Options,
    browser: &str,
    root: &std::path::Path,
    entries: &mut Vec<AutorunEntry>,
) {
    for kind in ["managed", "recommended"] {
        for path in list_files(&options.root, &root.join(kind)) {
            if path.extension().and_then(|value| value.to_str()) == Some("json") {
                scan_json_policy_file(options, browser, &path, entries);
            }
        }
    }
    let direct = root.join("policies.json");
    scan_json_policy_file(options, browser, &direct, entries);
}

fn scan_json_policy_file(
    options: &Options,
    browser: &str,
    path: &std::path::Path,
    entries: &mut Vec<AutorunEntry>,
) {
    if !path_exists(options, path) {
        return;
    }
    let Some(value) = read_json(options, path) else {
        return;
    };
    let policy = value.get("policies").unwrap_or(&value);
    if let Some(values) = policy
        .get("ExtensionInstallForcelist")
        .and_then(Value::as_array)
    {
        for value in values.iter().filter_map(Value::as_str) {
            emit_policy_entry(
                options,
                browser,
                path,
                value.split(';').next().unwrap_or(value),
                "force_installed",
                entries,
            );
        }
    }
    if let Some(settings) = policy.get("ExtensionSettings").and_then(Value::as_object) {
        for (id, setting) in settings {
            let mode = setting
                .get("installation_mode")
                .and_then(Value::as_str)
                .unwrap_or("configured");
            emit_policy_entry(options, browser, path, id, mode, entries);
        }
    }
    if let Some(installs) = policy
        .pointer("/Extensions/Install")
        .and_then(Value::as_array)
    {
        for value in installs.iter().filter_map(Value::as_str) {
            emit_policy_entry(options, browser, path, value, "force_installed", entries);
        }
    }
}

fn emit_policy_entry(
    options: &Options,
    browser: &str,
    path: &std::path::Path,
    id: &str,
    mode: &str,
    entries: &mut Vec<AutorunEntry>,
) {
    let mut entry = AutorunEntry::new(
        Category::Browser,
        id,
        display_location(path, &options.root),
        path.to_path_buf(),
    );
    entry.status = match mode {
        "blocked" | "removed" => EntryStatus::Disabled,
        "force_installed" | "normal_installed" => EntryStatus::Enabled,
        _ => EntryStatus::Conditional,
    };
    entry.event = Some("browser policy application and profile startup".to_string());
    entry.mechanism = Some("enterprise extension policy".to_string());
    entry.principal = Some("all users".to_string());
    entry.profile = Some(format!("{browser}/all profiles"));
    entry.activating_entity = Some(browser.to_string());
    entry.target = Some(id.to_string());
    entry.note = Some(format!("installation_mode={mode}"));
    entry.completeness = Some("supported extension policy keys parsed".to_string());
    entries.push(entry);
}

fn scan_external_extensions(
    options: &Options,
    browser: &str,
    dir: &std::path::Path,
    entries: &mut Vec<AutorunEntry>,
) {
    for path in list_files(&options.root, dir) {
        if path.extension().and_then(|value| value.to_str()) != Some("json") {
            continue;
        }
        let Some(value) = read_json(options, &path) else {
            continue;
        };
        let id = path
            .file_stem()
            .map(|value| value.to_string_lossy().to_string())
            .unwrap_or_else(|| "external extension".to_string());
        let target = value
            .get("external_crx")
            .or_else(|| value.get("external_update_url"))
            .and_then(Value::as_str)
            .unwrap_or(&id);
        let mut entry = AutorunEntry::new(
            Category::Browser,
            &id,
            display_location(&path, &options.root),
            path.clone(),
        );
        entry.status = EntryStatus::Conditional;
        entry.event = Some("browser startup external extension discovery".to_string());
        entry.mechanism = Some("external extension registration".to_string());
        entry.principal = Some("all users".to_string());
        entry.profile = Some(format!("{browser}/all profiles"));
        entry.activating_entity = Some(browser.to_string());
        entry.target = Some(target.to_string());
        entry.completeness = Some("external registration parsed".to_string());
        entries.push(entry);
    }
}

fn scan_launchers(
    options: &Options,
    principal: &str,
    dir: &std::path::Path,
    entries: &mut Vec<AutorunEntry>,
) {
    for path in list_files(&options.root, dir) {
        if path.extension().and_then(|value| value.to_str()) != Some("desktop") {
            continue;
        }
        let Some(content) = read_to_string(&options.root, &path) else {
            continue;
        };
        for command in content
            .lines()
            .filter_map(|line| line.trim().strip_prefix("Exec="))
        {
            let tokens = shell_tokens(command);
            let browser = browser_from_command(command);
            let mut index = 0;
            while index < tokens.len() {
                let value = if let Some(value) = tokens[index].strip_prefix("--load-extension=") {
                    Some(value.to_string())
                } else if tokens[index] == "--load-extension" {
                    tokens.get(index + 1).cloned()
                } else {
                    None
                };
                if let Some(value) = value {
                    for extension in value.split(',').filter(|value| !value.is_empty()) {
                        let mut entry = AutorunEntry::new(
                            Category::Browser,
                            std::path::Path::new(extension)
                                .file_name()
                                .map(|value| value.to_string_lossy().to_string())
                                .unwrap_or_else(|| "load-extension".to_string()),
                            display_location(&path, &options.root),
                            path.clone(),
                        );
                        entry.command = Some(command.to_string());
                        entry.status = EntryStatus::Enabled;
                        entry.event = Some("browser process launch".to_string());
                        entry.mechanism =
                            Some("persistent --load-extension launcher argument".to_string());
                        entry.principal = Some(principal.to_string());
                        entry.profile = Some(format!("{browser}/launcher-selected profile"));
                        entry.activating_entity = Some(browser.to_string());
                        entry.target = Some(extension.to_string());
                        entry.completeness = Some("persistent desktop launcher parsed".to_string());
                        entries.push(entry);
                    }
                }
                index += 1;
            }
        }
    }
}

fn scan_pkcs11(
    options: &Options,
    principal: &str,
    profile: &str,
    path: &std::path::Path,
    entries: &mut Vec<AutorunEntry>,
) {
    let Some(content) = read_to_string(&options.root, path) else {
        return;
    };
    let mut module_name = "PKCS #11 module".to_string();
    for line in content.lines().map(str::trim) {
        if let Some(value) = line.strip_prefix("name=") {
            module_name = value.to_string();
        } else if let Some(library) = line.strip_prefix("library=") {
            let mut entry = AutorunEntry::new(
                Category::Browser,
                &module_name,
                display_location(path, &options.root),
                path.to_path_buf(),
            );
            entry.image_path = Some(std::path::PathBuf::from(library));
            entry.status = EntryStatus::Conditional;
            entry.event = Some("browser cryptographic module initialization".to_string());
            entry.mechanism = Some("Firefox PKCS #11 module registry".to_string());
            entry.principal = Some(principal.to_string());
            entry.profile = Some(format!("Firefox/{profile}"));
            entry.activating_entity = Some("Firefox".to_string());
            entry.target = Some(library.to_string());
            entry.completeness = Some("registered native module parsed".to_string());
            entries.push(entry);
        }
    }
}

fn manifest_meta(
    options: &Options,
    path: &std::path::Path,
    cache: &mut HashMap<std::path::PathBuf, ExtensionMeta>,
) -> ExtensionMeta {
    if let Some(meta) = cache.get(path) {
        return meta.clone();
    }
    let meta = read_json(options, path)
        .as_ref()
        .map(meta_from_json)
        .unwrap_or_default();
    cache.insert(path.to_path_buf(), meta.clone());
    meta
}

fn xpi_meta(
    options: &Options,
    path: &std::path::Path,
    cache: &mut HashMap<std::path::PathBuf, ExtensionMeta>,
) -> ExtensionMeta {
    if let Some(meta) = cache.get(path) {
        return meta.clone();
    }
    let meta = (|| -> Result<ExtensionMeta, Box<dyn std::error::Error>> {
        let file = open_file_in_root(&options.root, path)?;
        let mut archive = zip::ZipArchive::new(file)?;
        let mut manifest = archive.by_name("manifest.json")?;
        let mut content = String::new();
        manifest.read_to_string(&mut content)?;
        let value: Value = serde_json::from_str(&content)?;
        Ok(meta_from_json(&value))
    })()
    .unwrap_or_else(|error| {
        record_diagnostic("parse browser extension archive", path, error);
        ExtensionMeta::default()
    });
    cache.insert(path.to_path_buf(), meta.clone());
    meta
}

fn meta_from_json(value: &Value) -> ExtensionMeta {
    ExtensionMeta {
        name: value
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        version: value
            .get("version")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        manifest_version: value
            .get("manifest_version")
            .and_then(Value::as_u64)
            .map(|value| value.to_string())
            .unwrap_or_default(),
    }
}

fn read_json(options: &Options, path: &std::path::Path) -> Option<Value> {
    let content = read_to_string(&options.root, path)?;
    match serde_json::from_str(&content) {
        Ok(value) => Some(value),
        Err(error) => {
            record_diagnostic("parse JSON", path, error);
            None
        }
    }
}

fn browser_from_command(command: &str) -> &'static str {
    let command = command.to_ascii_lowercase();
    if command.contains("chromium") {
        "Chromium"
    } else if command.contains("chrome") {
        "Google Chrome"
    } else if command.contains("edge") {
        "Microsoft Edge"
    } else if command.contains("brave") {
        "Brave"
    } else if command.contains("vivaldi") {
        "Vivaldi"
    } else if command.contains("opera") {
        "Opera"
    } else {
        "Chromium-family browser"
    }
}

fn chromium_policy_roots(options: &Options) -> Vec<(&'static str, std::path::PathBuf)> {
    [
        ("Google Chrome", "/etc/opt/chrome/policies"),
        ("Chromium", "/etc/chromium/policies"),
        ("Chromium", "/etc/chromium-browser/policies"),
        ("Microsoft Edge", "/etc/opt/edge/policies"),
        ("Brave", "/etc/brave/policies"),
    ]
    .into_iter()
    .map(|(browser, path)| (browser, rooted(options, path)))
    .collect()
}

fn system_native_host_dirs(options: &Options) -> Vec<(&'static str, std::path::PathBuf)> {
    [
        ("Google Chrome", "/etc/opt/chrome/native-messaging-hosts"),
        ("Chromium", "/etc/chromium/native-messaging-hosts"),
        ("Microsoft Edge", "/etc/opt/edge/native-messaging-hosts"),
        ("Firefox", "/usr/lib/mozilla/native-messaging-hosts"),
        ("Firefox", "/usr/lib64/mozilla/native-messaging-hosts"),
    ]
    .into_iter()
    .map(|(browser, path)| (browser, rooted(options, path)))
    .collect()
}

fn external_extension_dirs(options: &Options) -> Vec<(&'static str, std::path::PathBuf)> {
    [
        ("Google Chrome", "/opt/google/chrome/extensions"),
        ("Google Chrome", "/usr/share/google-chrome/extensions"),
        ("Chromium", "/usr/share/chromium/extensions"),
        ("Microsoft Edge", "/usr/share/microsoft-edge/extensions"),
    ]
    .into_iter()
    .map(|(browser, path)| (browser, rooted(options, path)))
    .collect()
}

fn path_exists(options: &Options, path: &std::path::Path) -> bool {
    path_is_file(&options.root, path)
}

fn is_dir(options: &Options, path: &std::path::Path) -> bool {
    path_is_dir(&options.root, path)
}
