use std::fmt;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Category {
    Logon,
    Services,
    ScheduledTasks,
    Boot,
    Hijacks,
    Loader,
    Network,
    Unsupported,
}

impl Category {
    pub fn implemented() -> Vec<Self> {
        vec![
            Self::Logon,
            Self::Services,
            Self::ScheduledTasks,
            Self::Boot,
            Self::Hijacks,
            Self::Loader,
            Self::Network,
        ]
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Logon => "Logon",
            Self::Services => "Services",
            Self::ScheduledTasks => "Scheduled Tasks",
            Self::Boot => "Boot Execute",
            Self::Hijacks => "Image Hijacks",
            Self::Loader => "Known DLLs / Loader",
            Self::Network => "Network Providers",
            Self::Unsupported => "Unsupported",
        }
    }
}

impl fmt::Display for Category {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.label())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryStatus {
    Enabled,
    Disabled,
    Unknown,
    Unsupported,
}

impl fmt::Display for EntryStatus {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            Self::Enabled => "enabled",
            Self::Disabled => "disabled",
            Self::Unknown => "unknown",
            Self::Unsupported => "unsupported",
        };
        formatter.write_str(value)
    }
}

#[derive(Debug, Clone)]
pub struct AutorunEntry {
    pub category: Category,
    pub name: String,
    pub description: Option<String>,
    pub publisher: Option<String>,
    pub image_path: Option<PathBuf>,
    pub command: Option<String>,
    pub location: String,
    pub source_path: PathBuf,
    pub status: EntryStatus,
    pub timestamp: Option<String>,
    pub sha256: Option<String>,
    pub note: Option<String>,
}

impl AutorunEntry {
    pub fn new(
        category: Category,
        name: impl Into<String>,
        location: impl Into<String>,
        source_path: PathBuf,
    ) -> Self {
        Self {
            category,
            name: name.into(),
            description: None,
            publisher: None,
            image_path: None,
            command: None,
            location: location.into(),
            source_path,
            status: EntryStatus::Unknown,
            timestamp: None,
            sha256: None,
            note: None,
        }
    }

    pub fn unsupported(
        category: Category,
        name: impl Into<String>,
        note: impl Into<String>,
    ) -> Self {
        Self {
            category,
            name: name.into(),
            description: None,
            publisher: None,
            image_path: None,
            command: None,
            location: "not applicable".to_string(),
            source_path: PathBuf::new(),
            status: EntryStatus::Unsupported,
            timestamp: None,
            sha256: None,
            note: Some(note.into()),
        }
    }
}
