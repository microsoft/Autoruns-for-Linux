use crate::model::AutorunEntry;

pub fn table(entries: &[AutorunEntry]) -> String {
    let mut output = String::new();
    output.push_str("Category\tStatus\tName\tImage Path\tLocation\tSource\tCommand\tNote\n");
    for entry in entries {
        output.push_str(&format!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
            entry.category,
            entry.status,
            entry.name,
            path_value(entry.image_path.as_ref()),
            entry.location,
            entry.source_path.display(),
            entry.command.as_deref().unwrap_or_default(),
            entry.note.as_deref().unwrap_or_default()
        ));
    }
    output
}

pub fn delimited(entries: &[AutorunEntry], delimiter: char) -> String {
    let mut output = String::new();
    let headers = ["Category", "Status", "Name", "Description", "Publisher", "ImagePath", "Command", "Location", "Source", "Timestamp", "SHA256", "Note"];
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
            entry.source_path.display().to_string(),
            entry.timestamp.clone().unwrap_or_default(),
            entry.sha256.clone().unwrap_or_default(),
            entry.note.clone().unwrap_or_default(),
        ];
        write_record(&mut output, delimiter, &values);
    }
    output
}

pub fn json(entries: &[AutorunEntry]) -> String {
    let mut output = String::from("[\n");
    for (index, entry) in entries.iter().enumerate() {
        if index > 0 {
            output.push_str(",\n");
        }
        output.push_str("  {");
        output.push_str(&json_field("category", &entry.category.to_string(), true));
        output.push_str(&json_field("status", &entry.status.to_string(), false));
        output.push_str(&json_field("name", &entry.name, false));
        output.push_str(&json_field("description", entry.description.as_deref().unwrap_or_default(), false));
        output.push_str(&json_field("publisher", entry.publisher.as_deref().unwrap_or_default(), false));
        output.push_str(&json_field("imagePath", &path_value(entry.image_path.as_ref()), false));
        output.push_str(&json_field("command", entry.command.as_deref().unwrap_or_default(), false));
        output.push_str(&json_field("location", &entry.location, false));
        output.push_str(&json_field("source", &entry.source_path.display().to_string(), false));
        output.push_str(&json_field("timestamp", entry.timestamp.as_deref().unwrap_or_default(), false));
        output.push_str(&json_field("sha256", entry.sha256.as_deref().unwrap_or_default(), false));
        output.push_str(&json_field("note", entry.note.as_deref().unwrap_or_default(), false));
        output.push_str("\n  }");
    }
    output.push_str("\n]\n");
    output
}

pub fn xml(entries: &[AutorunEntry]) -> String {
    let mut output = String::from("<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<autoruns>\n");
    for entry in entries {
        output.push_str("  <entry>\n");
        output.push_str(&xml_element("category", &entry.category.to_string()));
        output.push_str(&xml_element("status", &entry.status.to_string()));
        output.push_str(&xml_element("name", &entry.name));
        output.push_str(&xml_element("imagePath", &path_value(entry.image_path.as_ref())));
        output.push_str(&xml_element("command", entry.command.as_deref().unwrap_or_default()));
        output.push_str(&xml_element("location", &entry.location));
        output.push_str(&xml_element("source", &entry.source_path.display().to_string()));
        output.push_str(&xml_element("note", entry.note.as_deref().unwrap_or_default()));
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
    if value.contains(delimiter) || value.contains('"') || value.contains('\n') || value.contains('\r') {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
    }
}

fn json_field(name: &str, value: &str, first: bool) -> String {
    let prefix = if first { "\n" } else { ",\n" };
    format!("{prefix}    \"{}\": \"{}\"", escape_json(name), escape_json(value))
}

fn xml_element(name: &str, value: &str) -> String {
    format!("    <{name}>{}</{name}>\n", escape_xml(value))
}

fn path_value(path: Option<&std::path::PathBuf>) -> String {
    path.map(|path| path.display().to_string()).unwrap_or_default()
}

fn escape_json(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t")
}

fn escape_xml(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}