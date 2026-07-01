#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProvisionedPathKind {
    Directory,
    File,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProvisionedPathSecurity {
    Default,
    RegistryBinary(Vec<u8>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProvisionedPath {
    pub name: String,
    pub kind: ProvisionedPathKind,
    pub path: String,
    pub security: ProvisionedPathSecurity,
    pub required: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProvisionedPathRegistrySnapshot {
    pub entries: Vec<ProvisionedPath>,
    pub warnings: Vec<ProvisionedPathRegistryWarning>,
}

impl ProvisionedPathRegistrySnapshot {
    pub fn empty() -> Self {
        Self {
            entries: Vec::new(),
            warnings: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProvisionedPathRegistryWarning {
    pub entry: String,
    pub message: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProvisionedPathApplyReport {
    pub applied: Vec<String>,
    pub warnings: Vec<ProvisionedPathApplyFailure>,
    pub required_failures: Vec<ProvisionedPathApplyFailure>,
}

impl ProvisionedPathApplyReport {
    pub fn has_required_failures(&self) -> bool {
        !self.required_failures.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProvisionedPathApplyFailure {
    pub entry: String,
    pub path: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceRuntimeDirectory {
    pub name: String,
}
