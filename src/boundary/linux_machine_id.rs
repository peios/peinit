use std::io;
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};

#[cfg(feature = "peios-boundary")]
use peios::file::{CreateOptions, Disposition, FileAccess, OpenOptions, SecInfo};

pub const DEFAULT_MACHINE_ID_PATH: &str = "/lcl/etc/machine-id";

const MACHINE_ID_RANDOM_BYTES: usize = 16;
const MACHINE_ID_TEXT_BYTES: usize = 33;
const MACHINE_ID_READ_LIMIT: usize = 1024;
const HEX: &[u8; 16] = b"0123456789abcdef";

#[cfg(feature = "peios-boundary")]
const MACHINE_ID_FILE_SDDL: &str = "O:SYG:SYD:(A;;GA;;;SY)(A;;GA;;;BA)(A;;FR;;;BU)";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinuxMachineIdStatus {
    Existing,
    Generated,
    ReplacedInvalid,
    /// The identifier could not be read or persisted, so this boot uses one
    /// that will not survive it.
    ///
    /// §2.1 gives this step no recovery path — "invalid non-empty contents
    /// MUST be retained or logged as warning evidence, but MUST NOT enter
    /// Recovery mode" — and the machine ID is "a local opaque install ID"
    /// which "MUST NOT be treated as a security principal, credential, SID,
    /// account, or authorization input". Failing a boot over it is
    /// disproportionate to what it is for (PEI-363).
    Ephemeral { reason: String },
}

#[derive(Debug)]
pub enum LinuxMachineIdError {
    Read { path: PathBuf, source: io::Error },
    Generate { source: io::Error },
    Write { path: PathBuf, source: io::Error },
}

pub fn ensure_linux_machine_id() -> Result<LinuxMachineIdStatus, LinuxMachineIdError> {
    let mut syscalls = LinuxMachineIdSyscalls;
    ensure_linux_machine_id_with_syscalls(Path::new(DEFAULT_MACHINE_ID_PATH), &mut syscalls)
}

fn ensure_linux_machine_id_with_syscalls<S>(
    path: &Path,
    syscalls: &mut S,
) -> Result<LinuxMachineIdStatus, LinuxMachineIdError>
where
    S: LinuxMachineIdSyscallApi + ?Sized,
{
    // A read failure is not a reason to stop. It leaves peinit unable to say
    // whether a valid identifier is on disk, which is the same position as
    // finding none — so it takes the same path, and the boot continues with
    // whatever it can manage.
    let mut read_failure = None;
    let file_state = match syscalls.read_machine_id(path, MACHINE_ID_READ_LIMIT + 1) {
        Ok(Some(bytes)) => classify_machine_id_file(&bytes),
        Ok(None) => MachineIdFileState::Missing,
        Err(source) => {
            read_failure = Some(format!("read {} failed: {source}", path.display()));
            MachineIdFileState::Missing
        }
    };

    if file_state == MachineIdFileState::Valid {
        return Ok(LinuxMachineIdStatus::Existing);
    }

    // The one arm that does justify recovery. If the kernel cannot produce
    // random bytes, nothing else on the machine can be trusted either.
    let machine_id =
        generate_machine_id(syscalls).map_err(|source| LinuxMachineIdError::Generate { source })?;
    let text = encode_machine_id(machine_id);
    if let Err(source) = syscalls.write_machine_id_atomically(path, &text) {
        return Ok(LinuxMachineIdStatus::Ephemeral {
            reason: format!("write {} failed: {source}", path.display()),
        });
    }
    if let Some(reason) = read_failure {
        // Written, but peinit could not read what was there before, so it may
        // have replaced a valid identifier. Worth saying.
        return Ok(LinuxMachineIdStatus::Ephemeral { reason });
    }

    Ok(match file_state {
        MachineIdFileState::Missing | MachineIdFileState::Empty => LinuxMachineIdStatus::Generated,
        MachineIdFileState::Invalid => LinuxMachineIdStatus::ReplacedInvalid,
        MachineIdFileState::Valid => unreachable!("valid file returned early"),
    })
}

fn generate_machine_id<S>(syscalls: &mut S) -> io::Result<[u8; MACHINE_ID_RANDOM_BYTES]>
where
    S: LinuxMachineIdSyscallApi + ?Sized,
{
    let mut id = [0_u8; MACHINE_ID_RANDOM_BYTES];
    for _ in 0..8 {
        syscalls.read_kernel_random(&mut id)?;
        if id.iter().any(|byte| *byte != 0) {
            return Ok(id);
        }
    }
    Err(io::Error::new(
        io::ErrorKind::InvalidData,
        "kernel returned all-zero machine-id bytes",
    ))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MachineIdFileState {
    Missing,
    Empty,
    Valid,
    Invalid,
}

fn classify_machine_id_file(bytes: &[u8]) -> MachineIdFileState {
    if bytes.is_empty() || bytes.iter().all(|byte| byte.is_ascii_whitespace()) {
        return MachineIdFileState::Empty;
    }
    if bytes.len() != MACHINE_ID_TEXT_BYTES || bytes[MACHINE_ID_TEXT_BYTES - 1] != b'\n' {
        return MachineIdFileState::Invalid;
    }
    let id = &bytes[..MACHINE_ID_RANDOM_BYTES * 2];
    if id.iter().all(|byte| *byte == b'0') {
        return MachineIdFileState::Invalid;
    }
    if id
        .iter()
        .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        MachineIdFileState::Valid
    } else {
        MachineIdFileState::Invalid
    }
}

fn encode_machine_id(id: [u8; MACHINE_ID_RANDOM_BYTES]) -> [u8; MACHINE_ID_TEXT_BYTES] {
    let mut text = [0_u8; MACHINE_ID_TEXT_BYTES];
    for (index, byte) in id.iter().enumerate() {
        text[index * 2] = HEX[(byte >> 4) as usize];
        text[index * 2 + 1] = HEX[(byte & 0x0f) as usize];
    }
    text[MACHINE_ID_TEXT_BYTES - 1] = b'\n';
    text
}

trait LinuxMachineIdSyscallApi {
    fn read_machine_id(&mut self, path: &Path, limit: usize) -> io::Result<Option<Vec<u8>>>;
    fn read_kernel_random(&mut self, out: &mut [u8]) -> io::Result<()>;
    fn write_machine_id_atomically(&mut self, path: &Path, bytes: &[u8]) -> io::Result<()>;
}

#[derive(Debug, Clone, Copy, Default)]
struct LinuxMachineIdSyscalls;

impl LinuxMachineIdSyscallApi for LinuxMachineIdSyscalls {
    fn read_machine_id(&mut self, path: &Path, limit: usize) -> io::Result<Option<Vec<u8>>> {
        let file = match open_read_file(path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error),
        };
        read_fd_with_limit(file.as_raw_fd(), limit).map(Some)
    }

    fn read_kernel_random(&mut self, out: &mut [u8]) -> io::Result<()> {
        getrandom_fill(out)
    }

    fn write_machine_id_atomically(&mut self, path: &Path, bytes: &[u8]) -> io::Result<()> {
        write_machine_id_atomically(path, bytes)
    }
}

#[cfg(feature = "peios-boundary")]
fn open_read_file(path: &Path) -> io::Result<peios::file::File> {
    OpenOptions::new()
        .desired_access(FileAccess::READ_DATA)
        .open(None, path)
        .map_err(io::Error::from)
}

#[cfg(not(feature = "peios-boundary"))]
fn open_read_file(path: &Path) -> io::Result<std::fs::File> {
    std::fs::File::open(path)
}

fn getrandom_fill(mut out: &mut [u8]) -> io::Result<()> {
    while !out.is_empty() {
        let read = unsafe { libc::getrandom(out.as_mut_ptr().cast(), out.len(), 0) };
        if read < 0 {
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(error);
        }
        if read == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "getrandom returned zero bytes",
            ));
        }
        let read = read as usize;
        out = &mut out[read..];
    }
    Ok(())
}

fn read_fd_with_limit(fd: i32, limit: usize) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    let mut buffer = [0_u8; 128];
    while bytes.len() < limit {
        let capacity = (limit - bytes.len()).min(buffer.len());
        let read = unsafe { libc::read(fd, buffer.as_mut_ptr().cast(), capacity) };
        if read < 0 {
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(error);
        }
        if read == 0 {
            break;
        }
        bytes.extend_from_slice(&buffer[..read as usize]);
    }
    Ok(bytes)
}

fn write_machine_id_atomically(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let parent = path.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("machine-id path has no parent: {}", path.display()),
        )
    })?;
    // Step 4 runs before the path-provisioning machinery (§2.1 step 7) that
    // exists to make directories exist, and `/lcl` is a StrataFS view the
    // initramfs assembled — so whether `/lcl/etc/` is there is a property of
    // the image, not of peinit. Creating it here is the difference between a
    // first boot and a recovery shell (PEI-363).
    std::fs::create_dir_all(parent)?;
    let temp_path = temp_machine_id_path(path);
    let result = write_machine_id_temp_and_rename(path, &temp_path, parent, bytes);
    if result.is_err() {
        let _ = std::fs::remove_file(&temp_path);
    }
    result
}

fn write_machine_id_temp_and_rename(
    path: &Path,
    temp_path: &Path,
    parent: &Path,
    bytes: &[u8],
) -> io::Result<()> {
    let file = create_machine_id_temp_file(temp_path)?;
    write_all_fd(file.as_raw_fd(), bytes)?;
    fsync_fd(file.as_raw_fd())?;
    drop(file);
    std::fs::rename(temp_path, path)?;
    fsync_directory(parent)
}

fn temp_machine_id_path(path: &Path) -> PathBuf {
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("machine-id");
    path.with_file_name(format!(".{file_name}.tmp.{}", std::process::id()))
}

#[cfg(feature = "peios-boundary")]
fn create_machine_id_temp_file(path: &Path) -> io::Result<peios::file::File> {
    let sd = machine_id_file_sd()?;
    let file = OpenOptions::new()
        .desired_access(
            FileAccess::WRITE_DATA
                | FileAccess::READ_DATA
                | FileAccess::WRITE_DAC
                | FileAccess::WRITE_OWNER
                | FileAccess::SYNCHRONIZE,
        )
        .disposition(Disposition::OverwriteIf)
        .creator_sd(&sd)
        .open(None, path)
        .map_err(io::Error::from)?;
    file.fd_set_sd(SecInfo::OWNER | SecInfo::GROUP | SecInfo::DACL, &sd)
        .map_err(io::Error::from)?;
    Ok(file)
}

#[cfg(not(feature = "peios-boundary"))]
fn create_machine_id_temp_file(path: &Path) -> io::Result<std::fs::File> {
    std::fs::OpenOptions::new()
        .create(true)
        .truncate(true)
        .read(true)
        .write(true)
        .open(path)
}

#[cfg(feature = "peios-boundary")]
fn machine_id_file_sd() -> io::Result<peios::security::SecurityDescriptor> {
    peios::security::sddl::parse(MACHINE_ID_FILE_SDDL).map_err(io::Error::from)
}

fn write_all_fd(fd: i32, mut bytes: &[u8]) -> io::Result<()> {
    while !bytes.is_empty() {
        let written = unsafe { libc::write(fd, bytes.as_ptr().cast(), bytes.len()) };
        if written < 0 {
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(error);
        }
        if written == 0 {
            return Err(io::Error::new(
                io::ErrorKind::WriteZero,
                "write returned zero bytes",
            ));
        }
        bytes = &bytes[written as usize..];
    }
    Ok(())
}

fn fsync_fd(fd: i32) -> io::Result<()> {
    loop {
        let result = unsafe { libc::fsync(fd) };
        if result == 0 {
            return Ok(());
        }
        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::Interrupted {
            return Err(error);
        }
    }
}

fn fsync_directory(path: &Path) -> io::Result<()> {
    let dir = open_directory_for_fsync(path)?;
    match fsync_fd(dir.as_raw_fd()) {
        Ok(()) => Ok(()),
        Err(error) if error.raw_os_error() == Some(libc::EINVAL) => Ok(()),
        Err(error) => Err(error),
    }
}

#[cfg(feature = "peios-boundary")]
fn open_directory_for_fsync(path: &Path) -> io::Result<peios::file::File> {
    OpenOptions::new()
        .desired_access(FileAccess::READ_DATA | FileAccess::SYNCHRONIZE)
        .options(CreateOptions::DIRECTORY)
        .open(None, path)
        .map_err(io::Error::from)
}

#[cfg(not(feature = "peios-boundary"))]
fn open_directory_for_fsync(path: &Path) -> io::Result<std::fs::File> {
    std::fs::File::open(path)
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;

    use super::*;

    #[test]
    fn valid_existing_machine_id_is_preserved() {
        let mut syscalls = FakeMachineIdSyscalls {
            existing: Some(b"0123456789abcdef0123456789abcdef\n".to_vec()),
            ..FakeMachineIdSyscalls::default()
        };

        let status =
            ensure_linux_machine_id_with_syscalls(Path::new("/lcl/etc/machine-id"), &mut syscalls)
                .expect("ensure");

        assert_eq!(status, LinuxMachineIdStatus::Existing);
        assert!(syscalls.writes.is_empty());
    }

    #[test]
    fn missing_machine_id_is_generated_from_kernel_random() {
        let mut syscalls = FakeMachineIdSyscalls::with_random([[0xab; 16]]);

        let status =
            ensure_linux_machine_id_with_syscalls(Path::new("/lcl/etc/machine-id"), &mut syscalls)
                .expect("ensure");

        assert_eq!(status, LinuxMachineIdStatus::Generated);
        assert_eq!(
            syscalls.writes,
            vec![(
                "/lcl/etc/machine-id".to_string(),
                b"abababababababababababababababab\n".to_vec(),
            )],
        );
    }

    #[test]
    fn empty_machine_id_is_a_reset_marker_and_generates_new_id() {
        let mut syscalls = FakeMachineIdSyscalls::with_random([[0x42; 16]]);
        syscalls.existing = Some(Vec::new());

        let status =
            ensure_linux_machine_id_with_syscalls(Path::new("/lcl/etc/machine-id"), &mut syscalls)
                .expect("ensure");

        assert_eq!(status, LinuxMachineIdStatus::Generated);
        assert_eq!(
            syscalls.writes[0].1,
            b"42424242424242424242424242424242\n".to_vec(),
        );
    }

    #[test]
    fn invalid_machine_id_is_replaced_and_reported() {
        let mut syscalls = FakeMachineIdSyscalls::with_random([[0xcd; 16]]);
        syscalls.existing = Some(b"not-a-machine-id\n".to_vec());

        let status =
            ensure_linux_machine_id_with_syscalls(Path::new("/lcl/etc/machine-id"), &mut syscalls)
                .expect("ensure");

        assert_eq!(status, LinuxMachineIdStatus::ReplacedInvalid);
        assert_eq!(
            syscalls.writes[0].1,
            b"cdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcd\n".to_vec(),
        );
    }

    #[test]
    fn all_zero_random_bytes_are_rejected_and_retried() {
        let mut syscalls = FakeMachineIdSyscalls::with_random([[0; 16], [0x11; 16]]);

        ensure_linux_machine_id_with_syscalls(Path::new("/lcl/etc/machine-id"), &mut syscalls)
            .expect("ensure");

        assert_eq!(
            syscalls.writes[0].1,
            b"11111111111111111111111111111111\n".to_vec(),
        );
    }

    #[test]
    fn all_zero_existing_machine_id_is_invalid() {
        assert_eq!(
            classify_machine_id_file(b"00000000000000000000000000000000\n"),
            MachineIdFileState::Invalid,
        );
    }

    #[derive(Debug, Default)]
    struct FakeMachineIdSyscalls {
        existing: Option<Vec<u8>>,
        random: VecDeque<[u8; 16]>,
        writes: Vec<(String, Vec<u8>)>,
        read_error: Option<io::ErrorKind>,
        write_error: Option<io::ErrorKind>,
        random_error: bool,
    }

    impl FakeMachineIdSyscalls {
        fn with_random<const N: usize>(random: [[u8; 16]; N]) -> Self {
            Self {
                random: random.into(),
                ..Self::default()
            }
        }
    }

    impl LinuxMachineIdSyscallApi for FakeMachineIdSyscalls {
        fn read_machine_id(&mut self, _path: &Path, _limit: usize) -> io::Result<Option<Vec<u8>>> {
            if let Some(kind) = self.read_error {
                return Err(io::Error::new(kind, "read failed"));
            }
            Ok(self.existing.clone())
        }

        fn read_kernel_random(&mut self, out: &mut [u8]) -> io::Result<()> {
            if self.random_error {
                return Err(io::Error::other("no entropy"));
            }
            let next = self.random.pop_front().expect("random bytes");
            out.copy_from_slice(&next);
            Ok(())
        }

        fn write_machine_id_atomically(&mut self, path: &Path, bytes: &[u8]) -> io::Result<()> {
            if let Some(kind) = self.write_error {
                return Err(io::Error::new(kind, "write failed"));
            }
            self.writes
                .push((path.display().to_string(), bytes.to_vec()));
            Ok(())
        }
    }

    // PEI-363. §2.1 step 4 gives the machine ID no recovery path: "invalid
    // non-empty contents MUST be retained or logged as warning evidence, but
    // MUST NOT enter Recovery mode", and the failure table lists only
    // "generate/replace and continue". Any Err mapped to recovery, and the
    // write arm was reachable for a reason that has nothing to do with the
    // file's contents — an image shipping without `/lcl/etc/` present dropped
    // straight into a recovery shell on first boot.
    //
    // The value does not justify it either: §2.1 calls it "a local opaque
    // install ID" that "MUST NOT be treated as a security principal,
    // credential, SID, account, or authorization input".
    #[test]
    fn a_write_failure_yields_an_ephemeral_id_rather_than_recovery() {
        let mut syscalls = FakeMachineIdSyscalls::with_random([[0xab; 16]]);
        syscalls.write_error = Some(io::ErrorKind::PermissionDenied);

        let status =
            ensure_linux_machine_id_with_syscalls(Path::new("/lcl/etc/machine-id"), &mut syscalls)
                .expect("a write failure must not fail the boot");

        assert!(matches!(status, LinuxMachineIdStatus::Ephemeral { .. }));
    }

    /// A read failure leaves peinit unable to say whether a valid identifier
    /// is on disk — the same position as finding none, so it takes the same
    /// path. Ephemeral rather than Generated, because it may have replaced
    /// something valid and the operator should know.
    #[test]
    fn a_read_failure_generates_and_reports_rather_than_entering_recovery() {
        let mut syscalls = FakeMachineIdSyscalls::with_random([[0xab; 16]]);
        syscalls.read_error = Some(io::ErrorKind::PermissionDenied);

        let status =
            ensure_linux_machine_id_with_syscalls(Path::new("/lcl/etc/machine-id"), &mut syscalls)
                .expect("a read failure must not fail the boot");

        assert!(matches!(status, LinuxMachineIdStatus::Ephemeral { .. }));
        assert_eq!(syscalls.writes.len(), 1, "an identifier was still written");
    }

    /// The one arm where recovery is right. If the kernel cannot produce
    /// random bytes, nothing else on the machine can be trusted either.
    #[test]
    fn a_csprng_failure_is_still_fatal() {
        let mut syscalls = FakeMachineIdSyscalls {
            random_error: true,
            ..FakeMachineIdSyscalls::default()
        };

        let error =
            ensure_linux_machine_id_with_syscalls(Path::new("/lcl/etc/machine-id"), &mut syscalls)
                .expect_err("a CSPRNG failure is not survivable");

        assert!(matches!(error, LinuxMachineIdError::Generate { .. }));
    }
}
