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

/// Separator between a service's encoded cgroup id and its generation.
///
/// `%g` rather than `.`, because `.` is in the safe set and service names
/// permit it: `app.gen1` at generation 0 and `app` at generation 1 both
/// produced `/sys/fs/cgroup/peinit/app.gen1`, so a service could share a tree
/// with another service's leaked generation (PEI-353). The encoding's
/// injectivity argument covers the id and does not extend to a suffix appended
/// afterwards.
///
/// `%` self-escapes, and the encoder only ever emits it followed by two
/// *uppercase hex* digits — so `%g` cannot occur inside an encoded id, and its
/// first occurrence in a path separates id from generation unambiguously.
const GENERATION_SEPARATOR: &str = "%gen";

pub fn service_cgroup_root_path(service: &str, generation: u64) -> String {
    let encoded = encode_service_cgroup_id(service);
    if generation == 0 {
        format!("{PEINIT_CGROUP_ROOT}/{encoded}")
    } else {
        format!("{PEINIT_CGROUP_ROOT}/{encoded}{GENERATION_SEPARATOR}{generation}")
    }
}

pub fn service_job_cgroup_path(service: &str, generation: u64, kind: ServiceCgroupKind) -> String {
    format!(
        "{}/{}",
        service_cgroup_root_path(service, generation),
        kind.path_component()
    )
}

/// Every submitted job gets one cgroup of its own under `jobs/`, named by
/// its identifier rather than by anything a submitter chose: two submitters
/// naming the same description must never share a tree.
pub fn submitted_job_cgroup_path(job_id: JobId) -> String {
    format!("{PEINIT_CGROUP_ROOT}/jobs/{}", job_id.to_canonical_string())
}

fn is_cgroup_safe(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-')
}

const HEX_DIGITS: &[u8; 16] = b"0123456789ABCDEF";

/// Which of a service's processes a cgroup holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServicePart {
    /// The service's own process and everything it starts.
    Main,
    /// Its pre-exec, post-exec and reload hooks.
    Hooks,
    /// Its health checks.
    Health,
    /// Its pre-start checks.
    Checks,
}

/// What a process belongs to, by peinit's account, read back from the
/// cgroup peinit put it in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CgroupMember {
    /// One of a service's processes. `part` is `None` for a cgroup directly
    /// under the service's root, which peinit does not make.
    Service {
        service: String,
        part: Option<ServicePart>,
    },
    /// A submitted job's process, by the job's id as peinit prints it.
    Job(String),
}

/// What the contents of `/proc/<pid>/cgroup` say the process belongs to:
/// the inverse of [`service_job_cgroup_path`] and
/// [`submitted_job_cgroup_path`]. `None` for a process outside peinit's tree,
/// such as peinit itself or a kernel thread.
pub fn cgroup_member(proc_cgroup: &str) -> Option<CgroupMember> {
    let path = proc_cgroup.lines().find_map(|line| line.strip_prefix("0::"))?;
    let mut parts = path.strip_prefix("/peinit/")?.split('/');
    let first = parts.next()?;
    let second = parts.next();
    let part = |name: Option<&str>| match name {
        Some("main") => Some(ServicePart::Main),
        Some("hooks") => Some(ServicePart::Hooks),
        Some("health") => Some(ServicePart::Health),
        Some("checks") => Some(ServicePart::Checks),
        _ => None,
    };
    // `jobs/<id>`; a service named `jobs` has `main` and the like below it.
    if first == "jobs"
        && let Some(id) = second
        && part(Some(id)).is_none()
    {
        return Some(CgroupMember::Job(id.to_string()));
    }
    let encoded = first.split(GENERATION_SEPARATOR).next()?;
    Some(CgroupMember::Service {
        service: decode_service_cgroup_id(encoded)?,
        part: part(second),
    })
}

/// The inverse of [`encode_service_cgroup_id`].
fn decode_service_cgroup_id(encoded: &str) -> Option<String> {
    let bytes = encoded.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = encoded.get(i + 1..i + 3)?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}
