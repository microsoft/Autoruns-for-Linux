use std::io::Write;

use crate::{cli::OutputFormat, model::AutorunEntry};

const HEADERS: [&str; 22] = [
    "Category",
    "Status",
    "Name",
    "Description",
    "Publisher",
    "ImagePath",
    "Command",
    "Location",
    "Source",
    "Timestamp",
    "SHA256",
    "Note",
    "Event",
    "Mechanism",
    "Principal",
    "Profile",
    "Activator",
    "Target",
    "Completeness",
    "TargetState",
    "TargetExists",
    "TargetExecutable",
];

const CATEGORY_INDEX: usize = 0;
const NAME_INDEX: usize = 2;

pub fn write<W: Write>(
    output: &mut W,
    entries: &[AutorunEntry],
    format: &OutputFormat,
    root: &std::path::Path,
) -> std::io::Result<()> {
    match format {
        OutputFormat::Table => write_table(output, entries, root),
        OutputFormat::Csv => write_delimited(output, entries, ',', root),
        OutputFormat::Tsv => write_delimited(output, entries, '\t', root),
        OutputFormat::Json => write_json(output, entries, root),
        OutputFormat::Xml => write_xml(output, entries, root),
    }
}

fn write_table<W: Write>(
    output: &mut W,
    entries: &[AutorunEntry],
    root: &std::path::Path,
) -> std::io::Result<()> {
    let label_width = HEADERS
        .iter()
        .enumerate()
        .filter(|(index, _)| !is_heading_column(*index))
        .map(|(_, header)| header.chars().count())
        .max()
        .unwrap_or_default();

    let mut current_category: Option<String> = None;
    for entry in entries {
        let values = entry_values(entry, root).map(|value| terminal_safe(&value));
        let category = &values[CATEGORY_INDEX];

        if current_category.as_deref() != Some(category.as_str()) {
            if current_category.is_some() {
                output.write_all(b"\n")?;
            }
            writeln!(output, "{category}")?;
            current_category = Some(category.clone());
        }

        writeln!(output, "   {}", values[NAME_INDEX])?;
        for (index, label) in HEADERS.iter().enumerate() {
            let value = &values[index];
            if is_heading_column(index) || value.is_empty() {
                continue;
            }
            writeln!(output, "     {label:<label_width$}  {value}")?;
        }
    }
    Ok(())
}

/// Category heads each group and Name heads each entry, so neither repeats in the detail lines.
fn is_heading_column(index: usize) -> bool {
    index == CATEGORY_INDEX || index == NAME_INDEX
}

fn write_delimited<W: Write>(
    output: &mut W,
    entries: &[AutorunEntry],
    delimiter: char,
    root: &std::path::Path,
) -> std::io::Result<()> {
    write_record(output, delimiter, &HEADERS)?;
    for entry in entries {
        let values = entry_values(entry, root);
        write_record(output, delimiter, &values)?;
    }
    Ok(())
}

fn write_json<W: Write>(
    output: &mut W,
    entries: &[AutorunEntry],
    root: &std::path::Path,
) -> std::io::Result<()> {
    output.write_all(b"[\n")?;
    for (index, entry) in entries.iter().enumerate() {
        if index > 0 {
            output.write_all(b",\n")?;
        }
        output.write_all(b"  {")?;
        write_json_field(output, "category", &entry.category.to_string(), true)?;
        write_json_field(output, "status", &entry.status.to_string(), false)?;
        write_json_field(output, "name", &entry.name, false)?;
        write_json_field(
            output,
            "description",
            entry.description.as_deref().unwrap_or_default(),
            false,
        )?;
        write_json_field(
            output,
            "publisher",
            entry.publisher.as_deref().unwrap_or_default(),
            false,
        )?;
        write_json_field(
            output,
            "imagePath",
            &path_value(entry.image_path.as_ref()),
            false,
        )?;
        write_json_field(
            output,
            "command",
            entry.command.as_deref().unwrap_or_default(),
            false,
        )?;
        write_json_field(output, "location", &entry.location, false)?;
        write_json_field(
            output,
            "source",
            &source_value(&entry.source_path, root),
            false,
        )?;
        write_json_field(
            output,
            "timestamp",
            entry.timestamp.as_deref().unwrap_or_default(),
            false,
        )?;
        write_json_field(
            output,
            "sha256",
            entry.sha256.as_deref().unwrap_or_default(),
            false,
        )?;
        write_json_field(
            output,
            "note",
            entry.note.as_deref().unwrap_or_default(),
            false,
        )?;
        write_json_field(
            output,
            "event",
            entry.event.as_deref().unwrap_or_default(),
            false,
        )?;
        write_json_field(
            output,
            "mechanism",
            entry.mechanism.as_deref().unwrap_or_default(),
            false,
        )?;
        write_json_field(
            output,
            "principal",
            entry.principal.as_deref().unwrap_or_default(),
            false,
        )?;
        write_json_field(
            output,
            "profile",
            entry.profile.as_deref().unwrap_or_default(),
            false,
        )?;
        write_json_field(
            output,
            "activator",
            entry.activating_entity.as_deref().unwrap_or_default(),
            false,
        )?;
        write_json_field(
            output,
            "target",
            entry.target.as_deref().unwrap_or_default(),
            false,
        )?;
        write_json_field(
            output,
            "completeness",
            entry.completeness.as_deref().unwrap_or_default(),
            false,
        )?;
        write_json_field(
            output,
            "targetState",
            &entry
                .target_state
                .map(|state| state.to_string())
                .unwrap_or_default(),
            false,
        )?;
        write_json_optional_bool(output, "targetExists", entry.target_exists)?;
        write_json_optional_bool(output, "targetExecutable", entry.target_executable)?;
        output.write_all(b"\n  }")?;
    }
    output.write_all(b"\n]\n")
}

fn write_xml<W: Write>(
    output: &mut W,
    entries: &[AutorunEntry],
    root: &std::path::Path,
) -> std::io::Result<()> {
    output.write_all(b"<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<autoruns>\n")?;
    for entry in entries {
        output.write_all(b"  <entry>\n")?;
        write_xml_element(output, "category", &entry.category.to_string())?;
        write_xml_element(output, "status", &entry.status.to_string())?;
        write_xml_element(output, "name", &entry.name)?;
        write_xml_element(
            output,
            "description",
            entry.description.as_deref().unwrap_or_default(),
        )?;
        write_xml_element(
            output,
            "publisher",
            entry.publisher.as_deref().unwrap_or_default(),
        )?;
        write_xml_element(output, "imagePath", &path_value(entry.image_path.as_ref()))?;
        write_xml_element(
            output,
            "command",
            entry.command.as_deref().unwrap_or_default(),
        )?;
        write_xml_element(output, "location", &entry.location)?;
        write_xml_element(output, "source", &source_value(&entry.source_path, root))?;
        write_xml_element(
            output,
            "timestamp",
            entry.timestamp.as_deref().unwrap_or_default(),
        )?;
        write_xml_element(
            output,
            "sha256",
            entry.sha256.as_deref().unwrap_or_default(),
        )?;
        write_xml_element(output, "note", entry.note.as_deref().unwrap_or_default())?;
        write_xml_element(output, "event", entry.event.as_deref().unwrap_or_default())?;
        write_xml_element(
            output,
            "mechanism",
            entry.mechanism.as_deref().unwrap_or_default(),
        )?;
        write_xml_element(
            output,
            "principal",
            entry.principal.as_deref().unwrap_or_default(),
        )?;
        write_xml_element(
            output,
            "profile",
            entry.profile.as_deref().unwrap_or_default(),
        )?;
        write_xml_element(
            output,
            "activator",
            entry.activating_entity.as_deref().unwrap_or_default(),
        )?;
        write_xml_element(
            output,
            "target",
            entry.target.as_deref().unwrap_or_default(),
        )?;
        write_xml_element(
            output,
            "completeness",
            entry.completeness.as_deref().unwrap_or_default(),
        )?;
        write_xml_element(
            output,
            "targetState",
            &entry
                .target_state
                .map(|state| state.to_string())
                .unwrap_or_default(),
        )?;
        write_xml_element(output, "targetExists", &optional_bool(entry.target_exists))?;
        write_xml_element(
            output,
            "targetExecutable",
            &optional_bool(entry.target_executable),
        )?;
        output.write_all(b"  </entry>\n")?;
    }
    output.write_all(b"</autoruns>\n")
}

fn write_record<W: Write, S: AsRef<str>>(
    output: &mut W,
    delimiter: char,
    values: &[S],
) -> std::io::Result<()> {
    for (index, value) in values.iter().enumerate() {
        if index > 0 {
            write!(output, "{delimiter}")?;
        }
        output.write_all(escape_delimited(value.as_ref(), delimiter).as_bytes())?;
    }
    output.write_all(b"\n")
}

fn entry_values(entry: &AutorunEntry, root: &std::path::Path) -> [String; 22] {
    [
        entry.category.to_string(),
        entry.status.to_string(),
        entry.name.clone(),
        entry.description.clone().unwrap_or_default(),
        entry.publisher.clone().unwrap_or_default(),
        path_value(entry.image_path.as_ref()),
        entry.command.clone().unwrap_or_default(),
        entry.location.clone(),
        source_value(&entry.source_path, root),
        entry.timestamp.clone().unwrap_or_default(),
        entry.sha256.clone().unwrap_or_default(),
        entry.note.clone().unwrap_or_default(),
        entry.event.clone().unwrap_or_default(),
        entry.mechanism.clone().unwrap_or_default(),
        entry.principal.clone().unwrap_or_default(),
        entry.profile.clone().unwrap_or_default(),
        entry.activating_entity.clone().unwrap_or_default(),
        entry.target.clone().unwrap_or_default(),
        entry.completeness.clone().unwrap_or_default(),
        entry
            .target_state
            .map(|state| state.to_string())
            .unwrap_or_default(),
        optional_bool(entry.target_exists),
        optional_bool(entry.target_executable),
    ]
}

fn optional_bool(value: Option<bool>) -> String {
    value.map(|value| value.to_string()).unwrap_or_default()
}

fn terminal_safe(value: &str) -> String {
    let mut safe = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '\n' => safe.push_str("\\n"),
            '\r' => safe.push_str("\\r"),
            '\t' => safe.push_str("\\t"),
            value if value.is_control() => safe.push_str(&format!("\\u{{{:04x}}}", value as u32)),
            value => safe.push(value),
        }
    }
    safe
}

fn escape_delimited(value: &str, delimiter: char) -> String {
    let value = neutralize_formula(value);
    if value.contains(delimiter)
        || value.contains('"')
        || value.contains('\n')
        || value.contains('\r')
    {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value
    }
}

// Guards CSV/TSV exports against spreadsheet formula injection. A cell whose
// first non-space character is one that Excel/LibreOffice may treat as the start
// of a formula (`=`, `+`, `-`, `@`, or a leading tab/carriage return) is prefixed
// with a single quote so the spreadsheet renders it as literal text. Leading
// spaces are ignored when deciding, because some spreadsheets trim them and then
// interpret a value like " =1+1" as a formula. Scanned fields (paths, commands,
// desktop-entry names) come from untrusted filesystem content, so exported
// reports must be safe to open.
fn neutralize_formula(value: &str) -> String {
    match value.trim_start_matches(' ').chars().next() {
        Some('=') | Some('+') | Some('-') | Some('@') | Some('\t') | Some('\r') => {
            let mut safe = String::with_capacity(value.len() + 1);
            safe.push('\'');
            safe.push_str(value);
            safe
        }
        _ => value.to_string(),
    }
}

fn write_json_field<W: Write>(
    output: &mut W,
    name: &str,
    value: &str,
    first: bool,
) -> std::io::Result<()> {
    let prefix = if first { "\n" } else { ",\n" };
    write!(
        output,
        "{prefix}    \"{}\": \"{}\"",
        escape_json(name),
        escape_json(value)
    )
}

fn write_json_optional_bool<W: Write>(
    output: &mut W,
    name: &str,
    value: Option<bool>,
) -> std::io::Result<()> {
    let value = match value {
        Some(true) => "true",
        Some(false) => "false",
        None => "null",
    };
    write!(output, ",\n    \"{}\": {value}", escape_json(name),)
}

fn write_xml_element<W: Write>(output: &mut W, name: &str, value: &str) -> std::io::Result<()> {
    writeln!(output, "    <{name}>{}</{name}>", escape_xml(value))
}

fn path_value(path: Option<&std::path::PathBuf>) -> String {
    path.map(|path| path.display().to_string())
        .unwrap_or_default()
}

fn source_value(path: &std::path::Path, root: &std::path::Path) -> String {
    // Report the source as an absolute in-image path (leading `/`), consistent
    // with the Location and imagePath fields, rather than a root-relative one.
    crate::scanners::in_root_path(path, root)
        .display()
        .to_string()
}

fn escape_json(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '\\' => escaped.push_str("\\\\"),
            '"' => escaped.push_str("\\\""),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            '\u{0008}' => escaped.push_str("\\b"),
            '\u{000C}' => escaped.push_str("\\f"),
            // Any remaining C0 control character must be escaped as \u00XX to
            // keep the emitted JSON valid.
            c if (c as u32) < 0x20 => escaped.push_str(&format!("\\u{:04x}", c as u32)),
            c => escaped.push(c),
        }
    }
    escaped
}

fn escape_xml(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&apos;"),
            '\t' | '\n' | '\r' => escaped.push(ch),
            // XML 1.0 cannot represent the other C0 control characters, even as
            // numeric references, so drop them to keep the document valid.
            c if (c as u32) < 0x20 => {}
            c => escaped.push(c),
        }
    }
    escaped
}

#[cfg(test)]
mod tests {
    use super::escape_delimited;

    #[test]
    fn neutralizes_spreadsheet_formula_prefixes() {
        for value in [
            "=cmd()",
            "+1",
            "-1",
            "@SUM(A1)",
            "\tformula",
            "\rformula",
            " =1+1",
            "   -2",
        ] {
            let escaped = escape_delimited(value, ',');
            // The leading quote may itself be inside CSV quoting, so just assert
            // the neutralizing apostrophe precedes the original content.
            let unquoted = escaped.trim_matches('"');
            assert!(
                unquoted.starts_with('\''),
                "value {value:?} should be neutralized, got {escaped:?}"
            );
        }
    }

    #[test]
    fn leaves_ordinary_values_unquoted_and_unchanged() {
        assert_eq!(
            escape_delimited("/usr/bin/example", ','),
            "/usr/bin/example"
        );
        assert_eq!(escape_delimited("Example Name", ','), "Example Name");
    }

    #[test]
    fn still_quotes_embedded_delimiters_and_quotes() {
        assert_eq!(escape_delimited("a,b", ','), "\"a,b\"");
        assert_eq!(escape_delimited("a\"b", ','), "\"a\"\"b\"");
    }
}
