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
    Browser,
    DeviceMount,
    ApplicationIntegrations,
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
            Self::Browser,
            Self::DeviceMount,
            Self::ApplicationIntegrations,
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
            Self::Browser => "Browser Integrations",
            Self::DeviceMount => "Device / Mount Events",
            Self::ApplicationIntegrations => "Application Integrations",
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
    Conditional,
    Shadowed,
    Unknown,
    Error,
    Unsupported,
}

impl fmt::Display for EntryStatus {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            Self::Enabled => "enabled",
            Self::Disabled => "disabled",
            Self::Conditional => "conditional",
            Self::Shadowed => "shadowed",
            Self::Unknown => "unknown",
            Self::Error => "error",
            Self::Unsupported => "unsupported",
        };
        formatter.write_str(value)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetState {
    Present,
    Missing,
    Unresolved,
    Inaccessible,
}

impl fmt::Display for TargetState {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            Self::Present => "present",
            Self::Missing => "missing",
            Self::Unresolved => "unresolved",
            Self::Inaccessible => "inaccessible",
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
    pub event: Option<String>,
    pub mechanism: Option<String>,
    pub principal: Option<String>,
    pub profile: Option<String>,
    pub activating_entity: Option<String>,
    pub target: Option<String>,
    pub completeness: Option<String>,
    pub target_state: Option<TargetState>,
    pub target_exists: Option<bool>,
    pub target_executable: Option<bool>,
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
            event: None,
            mechanism: None,
            principal: None,
            profile: None,
            activating_entity: None,
            target: None,
            completeness: None,
            target_state: None,
            target_exists: None,
            target_executable: None,
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
            event: None,
            mechanism: None,
            principal: None,
            profile: None,
            activating_entity: None,
            target: None,
            completeness: Some("unsupported".to_string()),
            target_state: None,
            target_exists: None,
            target_executable: None,
        }
    }

    pub fn completeness_limit(
        category: Category,
        name: impl Into<String>,
        event: impl Into<String>,
        mechanism: impl Into<String>,
        note: impl Into<String>,
    ) -> Self {
        let mut entry = Self::unsupported(category, name, note);
        entry.event = Some(event.into());
        entry.mechanism = Some(mechanism.into());
        entry.completeness = Some("outside the supported static adapter set".to_string());
        entry
    }
}

#[derive(Debug, Clone)]
pub struct ScanDiagnostic {
    pub operation: String,
    pub path: PathBuf,
    pub message: String,
}

impl ScanDiagnostic {
    pub fn new(operation: impl Into<String>, path: PathBuf, message: impl Into<String>) -> Self {
        Self {
            operation: operation.into(),
            path,
            message: message.into(),
        }
    }
}
