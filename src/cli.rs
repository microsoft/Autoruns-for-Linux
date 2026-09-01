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
    pub utc_timestamps: bool,
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
            utc_timestamps: false,
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
            "-?" | "/?" | "--help" | "-help" => {
                options.show_help = true;
                index += 1;
            }
            "-a" | "/a" => {
                index += 1;
                let selectors = args
                    .get(index)
                    .ok_or("missing category selector after -a")?;
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
            "-m" | "/m" | "-s" | "/s" | "-u" | "/u" | "-v" | "/v" => {
                return Err(format!(
                    "unsupported security option: {} (publisher/signature and VirusTotal filtering are not implemented)",
                    args[index]
                ));
            }
            "-t" | "/t" => {
                options.utc_timestamps = true;
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
    if selectors.contains(',') || named_category(selectors).is_some() {
        let mut categories = Vec::new();
        for selector in selectors.split(',') {
            let selector = selector.trim();
            if selector.eq_ignore_ascii_case("all") || selector == "*" {
                return Ok(Category::implemented());
            }
            let category = named_category(selector)
                .ok_or_else(|| format!("unknown -a category: {selector}"))?;
            push_unique(&mut categories, category);
        }
        if categories.is_empty() {
            return Err("-a requires at least one selector".to_string());
        }
        return Ok(categories);
    }

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
            'i' => push_unique(&mut categories, Category::Browser),
            'o' => push_unique(&mut categories, Category::ApplicationIntegrations),
            'e' | 'm' | 'c' | 'd' | 'g' | 'p' | 'r' | 'w' | 'x' => {
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

fn named_category(selector: &str) -> Option<Category> {
    match selector.to_ascii_lowercase().as_str() {
        "logon" => Some(Category::Logon),
        "services" => Some(Category::Services),
        "scheduled" | "scheduled-tasks" | "timers" => Some(Category::ScheduledTasks),
        "boot" => Some(Category::Boot),
        "hijacks" => Some(Category::Hijacks),
        "loader" => Some(Category::Loader),
        "network" => Some(Category::Network),
        "browser" | "browsers" => Some(Category::Browser),
        "device" | "devices" | "mount" | "device-mount" => Some(Category::DeviceMount),
        "application" | "applications" | "apps" => Some(Category::ApplicationIntegrations),
        _ => None,
    }
}

fn push_unique(categories: &mut Vec<Category>, category: Category) {
    if !categories.contains(&category) {
        categories.push(category);
    }
}

pub fn usage() -> &'static str {
    "Autoruns for Linux shows programs configured to run automatically.\n\nUsage: autoruns [-a <*|blnsthkio|named[,named...]>] [-c|-ct|--json|-x] [-h] [-t] [-o <output file>] [--root <path>] [-nobanner]\n\n  -a   Autostart entry selection:\n       *    All implemented Linux categories.\n       b    Boot hooks.\n       h    Image hijacks and preload hooks.\n       i    Browser integrations.\n       k    Dynamic loader hooks.\n       l    Logon startups (default).\n       n    Network hooks.\n       o    Application integrations.\n       s    Services and module startup entries.\n       t    Scheduled tasks.\n       Named categories include browser, device, applications, and the names above.\n  -c     Print output as CSV.\n  -ct    Print output as tab-delimited values.\n  --json Print output as JSON.\n  -x     Print output as XML.\n  -h     Show SHA-256 hashes where the target file can be opened safely.\n  -t     Show timestamps in UTC where available.\n  -o     Write output to an owner-only file.\n  -m/-s/-u/-v\n         Unsupported security filters fail closed until implemented.\n  --root Scan an alternate filesystem root.\n  -nobanner\n         Do not display the startup banner."
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
        let options = parse_args(vec![
            "autoruns".to_string(),
            "-a".to_string(),
            "lst".to_string(),
        ])
        .unwrap();
        assert_eq!(
            options.categories,
            vec![
                Category::Logon,
                Category::Services,
                Category::ScheduledTasks
            ]
        );
    }

    #[test]
    fn accepts_windows_help_alias() {
        for flag in ["-?", "/?", "--help", "-help"] {
            let options = parse_args(vec!["autoruns".to_string(), flag.to_string()]).unwrap();
            assert!(options.show_help, "{flag} should request help");
        }
    }

    #[test]
    fn parses_named_linux_categories() {
        let options = parse_args(vec![
            "autoruns".to_string(),
            "-a".to_string(),
            "browser,device,applications".to_string(),
        ])
        .unwrap();
        assert_eq!(
            options.categories,
            vec![
                Category::Browser,
                Category::DeviceMount,
                Category::ApplicationIntegrations
            ]
        );
    }

    #[test]
    fn unsupported_security_options_fail_closed() {
        for flag in ["-m", "-s", "-u", "-v"] {
            let error = parse_args(vec!["autoruns".to_string(), flag.to_string()])
                .expect_err("security option should fail");
            assert!(error.contains("unsupported security option"), "{error}");
        }
        let error = parse_args(vec!["autoruns".to_string(), "-verbose".to_string()])
            .expect_err("prefix typo should fail");
        assert!(error.contains("unknown option"), "{error}");
    }
}
