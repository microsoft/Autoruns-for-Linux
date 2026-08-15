use crate::model::AutorunEntry;

const TABLE_HEADERS: [&str; 6] = ["Category", "Status", "Name", "Command", "Location", "Note"];

pub fn table(entries: &[AutorunEntry]) -> String {
    let rows: Vec<[String; 6]> = entries
        .iter()
        .map(|entry| {
            [
                entry.category.to_string(),
                entry.status.to_string(),
                entry.name.clone(),
                entry
                    .command
                    .clone()
                    .or_else(|| {
                        entry
                            .image_path
                            .as_ref()
                            .map(|path| path.display().to_string())
                    })
                    .unwrap_or_default(),
                entry.location.clone(),
                entry.note.as_deref().unwrap_or_default().to_string(),
            ]
        })
        .collect();

    let mut widths = TABLE_HEADERS.map(|header| header.chars().count());
    for row in &rows {
        for (index, cell) in row.iter().enumerate() {
            widths[index] = widths[index].max(cell.chars().count());
        }
    }

    let mut output = String::new();
    write_row(&mut output, &TABLE_HEADERS, &widths);
    let rules: Vec<String> = widths.iter().map(|width| "-".repeat(*width)).collect();
    write_row(&mut output, &rules, &widths);
    for row in &rows {
        write_row(&mut output, row, &widths);
    }
    output
}

fn write_row<S: AsRef<str>>(output: &mut String, cells: &[S], widths: &[usize]) {
    let last = cells.len() - 1;
    for (index, cell) in cells.iter().enumerate() {
        let text = cell.as_ref();
        output.push_str(text);
        if index != last {
            let pad = widths[index].saturating_sub(text.chars().count()) + 2;
            output.push_str(&" ".repeat(pad));
        }
    }
    output.push('\n');
}

pub fn delimited(entries: &[AutorunEntry], delimiter: char, root: &std::path::Path) -> String {
    let mut output = String::new();
    let headers = [
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
    ];
    write_record(&mut output, delimiter, &headers);
    for entry in entries {
        let values = [
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
        ];
        write_record(&mut output, delimiter, &values);
    }
    output
}

pub fn json(entries: &[AutorunEntry], root: &std::path::Path) -> String {
    let mut output = String::from("[\n");
    for (index, entry) in entries.iter().enumerate() {
        if index > 0 {
            output.push_str(",\n");
        }
        output.push_str("  {");
        output.push_str(&json_field("category", &entry.category.to_string(), true));
        output.push_str(&json_field("status", &entry.status.to_string(), false));
        output.push_str(&json_field("name", &entry.name, false));
        output.push_str(&json_field(
            "description",
            entry.description.as_deref().unwrap_or_default(),
            false,
        ));
        output.push_str(&json_field(
            "publisher",
            entry.publisher.as_deref().unwrap_or_default(),
            false,
        ));
        output.push_str(&json_field(
            "imagePath",
            &path_value(entry.image_path.as_ref()),
            false,
        ));
        output.push_str(&json_field(
            "command",
            entry.command.as_deref().unwrap_or_default(),
            false,
        ));
        output.push_str(&json_field("location", &entry.location, false));
        output.push_str(&json_field(
            "source",
            &source_value(&entry.source_path, root),
            false,
        ));
        output.push_str(&json_field(
            "timestamp",
            entry.timestamp.as_deref().unwrap_or_default(),
            false,
        ));
        output.push_str(&json_field(
            "sha256",
            entry.sha256.as_deref().unwrap_or_default(),
            false,
        ));
        output.push_str(&json_field(
            "note",
            entry.note.as_deref().unwrap_or_default(),
            false,
        ));
        output.push_str("\n  }");
    }
    output.push_str("\n]\n");
    output
}

pub fn xml(entries: &[AutorunEntry], root: &std::path::Path) -> String {
    let mut output = String::from("<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<autoruns>\n");
    for entry in entries {
        output.push_str("  <entry>\n");
        output.push_str(&xml_element("category", &entry.category.to_string()));
        output.push_str(&xml_element("status", &entry.status.to_string()));
        output.push_str(&xml_element("name", &entry.name));
        output.push_str(&xml_element(
            "description",
            entry.description.as_deref().unwrap_or_default(),
        ));
        output.push_str(&xml_element(
            "publisher",
            entry.publisher.as_deref().unwrap_or_default(),
        ));
        output.push_str(&xml_element(
            "imagePath",
            &path_value(entry.image_path.as_ref()),
        ));
        output.push_str(&xml_element(
            "command",
            entry.command.as_deref().unwrap_or_default(),
        ));
        output.push_str(&xml_element("location", &entry.location));
        output.push_str(&xml_element(
            "source",
            &source_value(&entry.source_path, root),
        ));
        output.push_str(&xml_element(
            "timestamp",
            entry.timestamp.as_deref().unwrap_or_default(),
        ));
        output.push_str(&xml_element(
            "sha256",
            entry.sha256.as_deref().unwrap_or_default(),
        ));
        output.push_str(&xml_element(
            "note",
            entry.note.as_deref().unwrap_or_default(),
        ));
        output.push_str("  </entry>\n");
    }
    output.push_str("</autoruns>\n");
    output
}

fn write_record<S: AsRef<str>>(output: &mut String, delimiter: char, values: &[S]) {
    for (index, value) in values.iter().enumerate() {
        if index > 0 {
            output.push(delimiter);
        }
        output.push_str(&escape_delimited(value.as_ref(), delimiter));
    }
    output.push('\n');
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

fn json_field(name: &str, value: &str, first: bool) -> String {
    let prefix = if first { "\n" } else { ",\n" };
    format!(
        "{prefix}    \"{}\": \"{}\"",
        escape_json(name),
        escape_json(value)
    )
}

fn xml_element(name: &str, value: &str) -> String {
    format!("    <{name}>{}</{name}>\n", escape_xml(value))
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
