#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrivilegeNameError {
    pub name: String,
}

#[derive(Debug, Clone, Copy)]
pub struct PrivilegeSpec {
    pub name: &'static str,
    pub bit: u32,
}

impl PrivilegeSpec {
    pub fn mask(self) -> u64 {
        1u64 << self.bit
    }
}

const SUPPORTED_PRIVILEGES: &[PrivilegeSpec] = &[
    PrivilegeSpec {
        name: "SeCreateTokenPrivilege",
        bit: 0,
    },
    PrivilegeSpec {
        name: "SeAssignPrimaryTokenPrivilege",
        bit: 1,
    },
    PrivilegeSpec {
        name: "SeLockMemoryPrivilege",
        bit: 2,
    },
    PrivilegeSpec {
        name: "SeIncreaseQuotaPrivilege",
        bit: 3,
    },
    PrivilegeSpec {
        name: "SeTcbPrivilege",
        bit: 7,
    },
    PrivilegeSpec {
        name: "SeSecurityPrivilege",
        bit: 8,
    },
    PrivilegeSpec {
        name: "SeLoadDriverPrivilege",
        bit: 10,
    },
    PrivilegeSpec {
        name: "SeSystemtimePrivilege",
        bit: 12,
    },
    PrivilegeSpec {
        name: "SeProfileSingleProcessPrivilege",
        bit: 13,
    },
    PrivilegeSpec {
        name: "SeIncreaseBasePriorityPrivilege",
        bit: 14,
    },
    PrivilegeSpec {
        name: "SeBackupPrivilege",
        bit: 17,
    },
    PrivilegeSpec {
        name: "SeRestorePrivilege",
        bit: 18,
    },
    PrivilegeSpec {
        name: "SeShutdownPrivilege",
        bit: 19,
    },
    PrivilegeSpec {
        name: "SeDebugPrivilege",
        bit: 20,
    },
    PrivilegeSpec {
        name: "SeAuditPrivilege",
        bit: 21,
    },
    PrivilegeSpec {
        name: "SeChangeNotifyPrivilege",
        bit: 23,
    },
    PrivilegeSpec {
        name: "SeRemoteShutdownPrivilege",
        bit: 24,
    },
    PrivilegeSpec {
        name: "SeImpersonatePrivilege",
        bit: 29,
    },
    PrivilegeSpec {
        name: "SeCreateSymbolicLinkPrivilege",
        bit: 35,
    },
];

pub fn supported_privileges() -> &'static [PrivilegeSpec] {
    SUPPORTED_PRIVILEGES
}

pub fn privilege_request_mask(names: &[String]) -> Result<u64, PrivilegeNameError> {
    let mut mask = 0u64;
    for name in names {
        let Some(spec) = privilege_by_name(name) else {
            return Err(PrivilegeNameError { name: name.clone() });
        };
        mask |= spec.mask();
    }
    Ok(mask)
}

pub fn privilege_names_from_mask(mask: u64) -> Vec<String> {
    SUPPORTED_PRIVILEGES
        .iter()
        .filter(|privilege| mask & privilege.mask() != 0)
        .map(|privilege| privilege.name.to_string())
        .collect()
}

fn privilege_by_name(name: &str) -> Option<PrivilegeSpec> {
    SUPPORTED_PRIVILEGES
        .iter()
        .copied()
        .find(|privilege| privilege.name == name)
}
