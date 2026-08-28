use std::collections::HashSet;
use std::io::Read;

use quick_xml::events::Event;
use quick_xml::Reader;

use crate::{
    cli::Options,
    model::{AutorunEntry, Category, EntryStatus},
};

use super::{
    display_location, in_root_path, list_dirs, list_files, modified_timestamp, open_file_in_root,
    path_is_dir, read_to_string, record_diagnostic, rooted, user_homes,
};

#[derive(Default)]
struct XmlEvidence {
    name: String,
    identifier: String,
    version: String,
    components: Vec<String>,
    events: Vec<String>,
}

#[derive(Default)]
struct OxtEvidence {
    xml: XmlEvidence,
    native_members: Vec<String>,
    event_files: Vec<String>,
}

pub fn scan(options: &Options) -> Vec<AutorunEntry> {
    let mut entries = Vec::new();
    let mut roots = vec![
        (
            "LibreOffice",
            "all users".to_string(),
            rooted(options, "/usr/lib/libreoffice/share/extensions"),
        ),
        (
            "LibreOffice",
            "all users".to_string(),
            rooted(options, "/usr/lib64/libreoffice/share/extensions"),
        ),
        (
            "LibreOffice",
            "all users".to_string(),
            rooted(options, "/usr/share/libreoffice/share/extensions"),
        ),
        (
            "OpenOffice",
            "all users".to_string(),
            rooted(options, "/usr/lib/openoffice/share/extensions"),
        ),
        (
            "LibreOffice",
            "all users".to_string(),
            rooted(options, "/opt/libreoffice/share/extensions"),
        ),
        (
            "OpenOffice",
            "all users".to_string(),
            rooted(options, "/opt/openoffice/share/extensions"),
        ),
    ];
    for product in list_dirs(&options.root, &rooted(options, "/opt")) {
        let name = product
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if name.starts_with("libreoffice") {
            roots.push((
                "LibreOffice",
                "all users".to_string(),
                product.join("share/extensions"),
            ));
        } else if name.starts_with("openoffice") {
            roots.push((
                "OpenOffice",
                "all users".to_string(),
                product.join("share/extensions"),
            ));
        }
    }
    for user in user_homes(options) {
        for (product, path) in [
            ("LibreOffice", ".config/libreoffice"),
            ("OpenOffice", ".config/openoffice"),
            ("OpenOffice", ".openoffice"),
        ] {
            roots.push((product, user.principal.clone(), user.path.join(path)));
        }
    }

    let mut seen_roots = HashSet::new();
    for (product, principal, root) in roots {
        if !seen_roots.insert((product, principal.clone(), root.clone())) || !is_dir(options, &root)
        {
            continue;
        }
        scan_tree(options, product, &principal, &root, &mut entries);
    }
    entries
}

fn scan_tree(
    options: &Options,
    product: &str,
    principal: &str,
    root: &std::path::Path,
    entries: &mut Vec<AutorunEntry>,
) {
    let mut pending = vec![(root.to_path_buf(), 0usize)];
    let mut seen = HashSet::new();
    while let Some((dir, depth)) = pending.pop() {
        if depth > 8 || !seen.insert(dir.clone()) {
            continue;
        }
        for child in list_dirs(&options.root, &dir) {
            pending.push((child, depth + 1));
        }
        for path in list_files(&options.root, &dir) {
            let name = path
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or("");
            if path.extension().and_then(|value| value.to_str()) == Some("oxt") {
                scan_oxt(options, product, principal, &path, entries);
            } else if name == "description.xml" {
                scan_description(options, product, principal, &path, entries);
            } else if path.extension().and_then(|value| value.to_str()) == Some("components") {
                scan_component_file(options, product, principal, &path, entries);
            } else if matches!(
                name,
                "Jobs.xcu" | "Events.xcu" | "registrymodifications.xcu" | "script.xlb"
            ) {
                scan_event_file(options, product, principal, &path, entries);
            } else if path.extension().and_then(|value| value.to_str()) == Some("so") {
                entries.push(native_entry(options, product, principal, &path, name));
            }
        }
    }
}

fn scan_description(
    options: &Options,
    product: &str,
    principal: &str,
    path: &std::path::Path,
    entries: &mut Vec<AutorunEntry>,
) {
    let Some(content) = read_to_string(&options.root, path) else {
        return;
    };
    let Some(evidence) = parse_xml_evidence(path, &content) else {
        return;
    };
    let package = path.parent().unwrap_or(path);
    entries.push(package_entry(
        options,
        product,
        principal,
        path,
        &in_root_path(package, &options.root).display().to_string(),
        &evidence,
        "unpacked office extension",
    ));
}

fn scan_component_file(
    options: &Options,
    product: &str,
    principal: &str,
    path: &std::path::Path,
    entries: &mut Vec<AutorunEntry>,
) {
    let Some(content) = read_to_string(&options.root, path) else {
        return;
    };
    let Some(evidence) = parse_xml_evidence(path, &content) else {
        return;
    };
    if evidence.components.is_empty() {
        entries.push(component_entry(
            options,
            product,
            principal,
            path,
            "UNO component registry",
            None,
        ));
    } else {
        for component in evidence.components {
            entries.push(component_entry(
                options,
                product,
                principal,
                path,
                &component,
                Some(&component),
            ));
        }
    }
}

fn scan_event_file(
    options: &Options,
    product: &str,
    principal: &str,
    path: &std::path::Path,
    entries: &mut Vec<AutorunEntry>,
) {
    let Some(content) = read_to_string(&options.root, path) else {
        return;
    };
    let Some(evidence) = parse_xml_evidence(path, &content) else {
        return;
    };
    let name = path
        .file_name()
        .map(|value| value.to_string_lossy().to_string())
        .unwrap_or_else(|| "office event configuration".to_string());
    let mut entry = AutorunEntry::new(
        Category::ApplicationIntegrations,
        &name,
        display_location(path, &options.root),
        path.to_path_buf(),
    );
    entry.status = EntryStatus::Conditional;
    entry.timestamp = modified_timestamp(&options.root, path);
    entry.event = Some(if evidence.events.is_empty() {
        "office application or document event".to_string()
    } else {
        evidence.events.join("; ")
    });
    entry.mechanism = Some(
        if name == "script.xlb" {
            "office macro library registration"
        } else {
            "office event/job configuration"
        }
        .to_string(),
    );
    entry.principal = Some(principal.to_string());
    entry.profile = Some(product.to_string());
    entry.activating_entity = Some(product.to_string());
    entry.target = Some(display_location(path, &options.root));
    entry.note = Some("static integration evidence; macros and jobs were not executed".to_string());
    entry.completeness = Some("supported LibreOffice/OpenOffice event source parsed".to_string());
    entries.push(entry);
}

fn scan_oxt(
    options: &Options,
    product: &str,
    principal: &str,
    path: &std::path::Path,
    entries: &mut Vec<AutorunEntry>,
) {
    let parsed = (|| -> Result<OxtEvidence, Box<dyn std::error::Error>> {
        let file = open_file_in_root(&options.root, path)?;
        let mut archive = zip::ZipArchive::new(file)?;
        let mut evidence = OxtEvidence::default();
        for index in 0..archive.len() {
            let mut member = archive.by_index(index)?;
            let name = member.name().to_string();
            if name.ends_with(".so") {
                evidence.native_members.push(name.clone());
            }
            if name.ends_with("Jobs.xcu")
                || name.ends_with("Events.xcu")
                || name.ends_with("script.xlb")
            {
                evidence.event_files.push(name.clone());
            }
            if name == "description.xml"
                || name.ends_with(".components")
                || name.ends_with("Jobs.xcu")
                || name.ends_with("Events.xcu")
            {
                let mut content = String::new();
                member.read_to_string(&mut content)?;
                if let Some(parsed) = parse_xml_evidence(path, &content) {
                    if evidence.xml.name.is_empty() {
                        evidence.xml.name = parsed.name;
                    }
                    if evidence.xml.identifier.is_empty() {
                        evidence.xml.identifier = parsed.identifier;
                    }
                    if evidence.xml.version.is_empty() {
                        evidence.xml.version = parsed.version;
                    }
                    evidence.xml.components.extend(parsed.components);
                    evidence.xml.events.extend(parsed.events);
                }
            }
        }
        Ok(evidence)
    })();
    let evidence = match parsed {
        Ok(parsed) => parsed,
        Err(error) => {
            record_diagnostic("parse OXT archive", path, error);
            return;
        }
    };

    let target = display_location(path, &options.root);
    entries.push(package_entry(
        options,
        product,
        principal,
        path,
        &target,
        &evidence.xml,
        "OXT extension package",
    ));
    for event in evidence.xml.events {
        let mut entry = component_entry(options, product, principal, path, &event, None);
        entry.event = Some(event);
        entry.mechanism = Some("event binding inside OXT package".to_string());
        entry.target = Some(target.clone());
        entries.push(entry);
    }
    for component in evidence.xml.components {
        let mut entry = component_entry(options, product, principal, path, &component, None);
        entry.target = Some(format!("{target}!{component}"));
        entries.push(entry);
    }
    for native in evidence.native_members {
        let mut entry = component_entry(options, product, principal, path, &native, None);
        entry.mechanism = Some("native helper inside OXT package".to_string());
        entry.target = Some(format!("{target}!{native}"));
        entries.push(entry);
    }
    for event_file in evidence.event_files {
        let mut entry = component_entry(options, product, principal, path, &event_file, None);
        entry.event = Some("office application or document event".to_string());
        entry.mechanism = Some("event or macro registration inside OXT package".to_string());
        entry.target = Some(format!("{target}!{event_file}"));
        entries.push(entry);
    }
}

#[allow(clippy::too_many_arguments)]
fn package_entry(
    options: &Options,
    product: &str,
    principal: &str,
    source: &std::path::Path,
    target: &str,
    evidence: &XmlEvidence,
    mechanism: &str,
) -> AutorunEntry {
    let fallback = source
        .file_stem()
        .map(|value| value.to_string_lossy().to_string())
        .unwrap_or_else(|| "office extension".to_string());
    let mut entry = AutorunEntry::new(
        Category::ApplicationIntegrations,
        if evidence.name.is_empty() {
            fallback
        } else {
            evidence.name.clone()
        },
        display_location(source, &options.root),
        source.to_path_buf(),
    );
    entry.description =
        (!evidence.version.is_empty()).then(|| format!("version {}", evidence.version));
    entry.status = EntryStatus::Conditional;
    entry.timestamp = modified_timestamp(&options.root, source);
    entry.event = Some("office host extension initialization".to_string());
    entry.mechanism = Some(mechanism.to_string());
    entry.principal = Some(principal.to_string());
    entry.profile = Some(product.to_string());
    entry.activating_entity = Some(product.to_string());
    entry.target = Some(target.to_string());
    if !evidence.identifier.is_empty() {
        entry.note = Some(format!("identifier={}", evidence.identifier));
    }
    entry.completeness = Some("supported LibreOffice/OpenOffice adapter".to_string());
    entry
}

fn component_entry(
    options: &Options,
    product: &str,
    principal: &str,
    source: &std::path::Path,
    name: &str,
    target: Option<&str>,
) -> AutorunEntry {
    let mut entry = AutorunEntry::new(
        Category::ApplicationIntegrations,
        name,
        display_location(source, &options.root),
        source.to_path_buf(),
    );
    entry.status = EntryStatus::Conditional;
    entry.timestamp = modified_timestamp(&options.root, source);
    entry.event = Some("office component demand or host initialization".to_string());
    entry.mechanism = Some("UNO component registration".to_string());
    entry.principal = Some(principal.to_string());
    entry.profile = Some(product.to_string());
    entry.activating_entity = Some(product.to_string());
    entry.target = target.map(str::to_string);
    entry.completeness = Some("supported LibreOffice/OpenOffice adapter".to_string());
    entry
}

fn native_entry(
    options: &Options,
    product: &str,
    principal: &str,
    path: &std::path::Path,
    name: &str,
) -> AutorunEntry {
    let mut entry = component_entry(options, product, principal, path, name, None);
    let target = in_root_path(path, &options.root);
    entry.image_path = Some(target.clone());
    entry.target = Some(target.display().to_string());
    entry.mechanism = Some("office native extension helper".to_string());
    entry
}

fn parse_xml_evidence(path: &std::path::Path, content: &str) -> Option<XmlEvidence> {
    let parsed = (|| -> Result<XmlEvidence, String> {
        let mut reader = Reader::from_str(content);
        reader.config_mut().trim_text(true);
        let mut evidence = XmlEvidence::default();
        let mut current = String::new();
        let mut depth = 0usize;
        loop {
            match reader.read_event().map_err(|error| error.to_string())? {
                Event::Start(element) => {
                    depth += 1;
                    current = String::from_utf8_lossy(element.local_name().as_ref()).to_string();
                    parse_xml_attributes(&reader, &element, &current, &mut evidence)?;
                }
                Event::Empty(element) => {
                    current = String::from_utf8_lossy(element.local_name().as_ref()).to_string();
                    parse_xml_attributes(&reader, &element, &current, &mut evidence)?;
                }
                Event::End(_) => {
                    depth = depth
                        .checked_sub(1)
                        .ok_or_else(|| "unexpected XML closing element".to_string())?;
                }
                Event::Text(text) => {
                    let value = text
                        .decode()
                        .map_err(|error| error.to_string())?
                        .into_owned();
                    if current == "name" && evidence.name.is_empty() && !value.is_empty() {
                        evidence.name = value;
                    } else if current == "value" && !value.is_empty() {
                        evidence.events.push(value);
                    }
                }
                Event::Eof if depth == 0 => break,
                Event::Eof => {
                    return Err("unexpected end of XML with unclosed elements".to_string())
                }
                _ => {}
            }
        }
        evidence.components.sort();
        evidence.components.dedup();
        evidence.events.sort();
        evidence.events.dedup();
        Ok(evidence)
    })();
    match parsed {
        Ok(evidence) => Some(evidence),
        Err(error) => {
            record_diagnostic("parse XML", path, error);
            None
        }
    }
}

fn parse_xml_attributes(
    reader: &Reader<&[u8]>,
    element: &quick_xml::events::BytesStart<'_>,
    element_name: &str,
    evidence: &mut XmlEvidence,
) -> Result<(), String> {
    for attribute in element.attributes() {
        let attribute = attribute.map_err(|error| error.to_string())?;
        let key = String::from_utf8_lossy(attribute.key.local_name().as_ref()).to_string();
        let value = attribute
            .decode_and_unescape_value(reader.decoder())
            .map_err(|error| error.to_string())?
            .into_owned();
        match key.as_str() {
            "identifier" => evidence.identifier = value,
            "version" => evidence.version = value,
            "uri" | "loader" | "implementation" => {
                if !value.is_empty() {
                    evidence.components.push(value);
                }
            }
            "name" if element_name == "implementation" && !value.is_empty() => {
                evidence.components.push(value)
            }
            "name" if value.starts_with("On") || value.contains("Event") => {
                evidence.events.push(value)
            }
            _ => {}
        }
    }
    Ok(())
}

fn is_dir(options: &Options, path: &std::path::Path) -> bool {
    path_is_dir(&options.root, path)
}
