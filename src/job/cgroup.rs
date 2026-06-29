use crate::ids::JobId;

const PEINIT_CGROUP_ROOT: &str = "/sys/fs/cgroup/peinit";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceCgroupKind {
    Main,
    Hooks,
    Health,
}

impl ServiceCgroupKind {
    fn path_component(self) -> &'static str {
        match self {
            Self::Main => "main",
            Self::Hooks => "hooks",
            Self::Health => "health",
        }
    }
}

pub fn encode_service_cgroup_id(service: &str) -> String {
    let mut encoded = String::with_capacity(service.len());
    for byte in service.bytes() {
        if is_cgroup_safe(byte) {
            encoded.push(char::from(byte));
        } else {
            encoded.push('%');
            encoded.push(char::from(HEX_DIGITS[usize::from(byte >> 4)]));
            encoded.push(char::from(HEX_DIGITS[usize::from(byte & 0x0f)]));
        }
    }
    encoded
}

pub fn service_cgroup_root_path(service: &str, generation: u64) -> String {
    let encoded = encode_service_cgroup_id(service);
    if generation == 0 {
        format!("{PEINIT_CGROUP_ROOT}/{encoded}")
    } else {
        format!("{PEINIT_CGROUP_ROOT}/{encoded}.gen{generation}")
    }
}

pub fn service_job_cgroup_path(service: &str, generation: u64, kind: ServiceCgroupKind) -> String {
    format!(
        "{}/{}",
        service_cgroup_root_path(service, generation),
        kind.path_component()
    )
}

pub fn ad_hoc_cgroup_path(job_id: JobId) -> String {
    format!("{PEINIT_CGROUP_ROOT}/{}", job_id.to_canonical_string())
}

fn is_cgroup_safe(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-')
}

const HEX_DIGITS: &[u8; 16] = b"0123456789ABCDEF";
