use std::path::PathBuf;

use crate::model::Category;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutputFormat {
    Table,
    Csv,
    Tsv,
    Json,
    Xml,
}

#[derive(Debug, Clone)]
pub struct Options {
    pub categories: Vec<Category>,
    pub format: OutputFormat,
    pub output_file: Option<PathBuf>,
    pub root: PathBuf,
    pub show_hashes: bool,
    pub hide_microsoft: bool,
    pub verify_signatures: bool,
    pub utc_timestamps: bool,
    pub show_unsigned_only: bool,
    pub virus_total_check: bool,
    pub no_banner: bool,
    pub show_help: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            categories: vec![Category::Logon],
            format: OutputFormat::Table,
            output_file: None,
            root: PathBuf::from("/"),
            show_hashes: false,
            hide_microsoft: false,
            verify_signatures: false,
            utc_timestamps: false,
            show_unsigned_only: false,
            virus_total_check: false,
            no_banner: false,
            show_help: false,
        }
    }
}

pub fn parse_args(args: Vec<String>) -> Result<Options, String> {
    let mut options = Options::default();
    let mut index = 1;

    while index < args.len() {
        match args[index].as_str() {
            "-?" | "--help" | "-help" => {
                options.show_help = true;
                index += 1;
            }
            "-a" | "/a" => {
                index += 1;
                let selectors = args.get(index).ok_or("missing category selector after -a")?;
                options.categories = parse_categories(selectors)?;
                index += 1;
            }
            "-c" | "/c" => {
                options.format = OutputFormat::Csv;
                index += 1;
            }
            "-ct" | "/ct" => {
                options.format = OutputFormat::Tsv;
                index += 1;
            }
            "--json" => {
                options.format = OutputFormat::Json;
                index += 1;
            }
            "-x" | "/x" | "--xml" => {
                options.format = OutputFormat::Xml;
                index += 1;
            }
            "-h" | "/h" => {
                options.show_hashes = true;
                index += 1;
            }
            "-m" | "/m" => {
                options.hide_microsoft = true;
                index += 1;
            }
            "-s" | "/s" => {
                options.verify_signatures = true;
                index += 1;
            }
            "-t" | "/t" => {
                options.utc_timestamps = true;
                index += 1;
            }
            "-u" | "/u" => {
                options.verify_signatures = true;
                options.show_unsigned_only = true;
                index += 1;
            }
            "-o" | "/o" => {
                index += 1;
                let path = args.get(index).ok_or("missing output path after -o")?;
                options.output_file = Some(PathBuf::from(path));
                index += 1;
            }
            "--root" => {
                index += 1;
                let root = args.get(index).ok_or("missing path after --root")?;
                options.root = PathBuf::from(root);
                index += 1;
            }
            "-nobanner" | "/nobanner" | "--nobanner" => {
                options.no_banner = true;
                index += 1;
            }
            value if value.starts_with("-v") || value.starts_with("/v") => {
                options.virus_total_check = true;
                index += 1;
            }
            value if value.starts_with('-') || value.starts_with('/') => {
                return Err(format!("unknown option: {value}"));
            }
            value => {
                return Err(format!("unexpected positional argument: {value}"));
            }
        }
    }

    Ok(options)
}

fn parse_categories(selectors: &str) -> Result<Vec<Category>, String> {
    let mut categories = Vec::new();

    for selector in selectors.chars() {
        match selector.to_ascii_lowercase() {
            '*' => return Ok(Category::implemented()),
            'l' => push_unique(&mut categories, Category::Logon),
            's' => push_unique(&mut categories, Category::Services),
            't' => push_unique(&mut categories, Category::ScheduledTasks),
            'b' => push_unique(&mut categories, Category::Boot),
            'h' => push_unique(&mut categories, Category::Hijacks),
            'n' => push_unique(&mut categories, Category::Network),
            'k' => push_unique(&mut categories, Category::Loader),
            'e' | 'i' | 'o' | 'm' | 'c' | 'd' | 'g' | 'p' | 'r' | 'w' | 'x' => {
                push_unique(&mut categories, Category::Unsupported)
            }
            other => return Err(format!("unknown -a selector: {other}")),
        }
    }

    if categories.is_empty() {
        return Err("-a requires at least one selector".to_string());
    }

    Ok(categories)
}

fn push_unique(categories: &mut Vec<Category>, category: Category) {
    if !categories.contains(&category) {
        categories.push(category);
    }
}

pub fn usage() -> &'static str {
    "Autoruns for Linux shows programs configured to run automatically.\n\nUsage: autoruns [-a <*|blnsthk>] [-c|-ct|--json|-x] [-h] [-m] [-s] [-u] [-t] [-o <output file>] [--root <path>] [-nobanner]\n\n  -a   Autostart entry selection:\n       *    All implemented Linux categories.\n       b    Boot hooks.\n       h    Image hijacks and preload hooks.\n       k    Dynamic loader hooks.\n       l    Logon startups (default).\n       n    Network hooks.\n       s    Services and module startup entries.\n       t    Scheduled tasks.\n  -c     Print output as CSV.\n  -ct    Print output as tab-delimited values.\n  --json Print output as JSON.\n  -x     Print output as XML.\n  -h     Show file hashes where the target file can be resolved.\n  -m     Hide Microsoft entries (accepted for Windows parity; not implemented yet).\n  -o     Write output to the specified file.\n  -s     Verify digital signatures (accepted for Windows parity; not implemented yet).\n  -t     Show timestamps in UTC where available.\n  -u     Show unsigned only (accepted for Windows parity; not implemented yet).\n  --root Scan an alternate filesystem root.\n  -nobanner\n         Do not display the startup banner."
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_to_logon() {
        let options = parse_args(vec!["autoruns".to_string()]).unwrap();
        assert_eq!(options.categories, vec![Category::Logon]);
    }

    #[test]
    fn parses_multiple_categories() {
        let options = parse_args(vec!["autoruns".to_string(), "-a".to_string(), "lst".to_string()]).unwrap();
        assert_eq!(options.categories, vec![Category::Logon, Category::Services, Category::ScheduledTasks]);
    }
}