use std::io;
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};

#[cfg(feature = "peios-boundary")]
use peios::file::{Disposition, FileAccess, OpenOptions, SecInfo};

pub const DEFAULT_RANDOM_SEED_PATH: &str = "/var/state/peinit/random-seed";

const DEFAULT_RANDOM_DEVICE_PATH: &str = "/dev/urandom";
const RANDOM_SEED_BYTES: usize = 512;
const MAX_RESTORE_SEED_BYTES: usize = 4096;
const RNDADDENTROPY: libc::c_ulong = rndaddentropy_request();

#[cfg(feature = "peios-boundary")]
const RANDOM_SEED_DIR_SDDL: &str = "O:SYG:SYD:(A;OICI;GA;;;SY)";
#[cfg(feature = "peios-boundary")]
const RANDOM_SEED_FILE_SDDL: &str = "O:SYG:SYD:(A;;GA;;;SY)";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinuxRandomSeedRestoreStatus {
    Missing,
    Credited,
    MixedWithoutCredit { credit_error: String },
}

#[derive(Debug)]
pub enum LinuxRandomSeedError {
    ReadSeed {
        path: PathBuf,
        source: io::Error,
    },
    InvalidSeedSize {
        path: PathBuf,
        bytes: usize,
        max: usize,
    },
    CreditSeed {
        random_device_path: PathBuf,
        source: io::Error,
    },
    MixSeed {
        random_device_path: PathBuf,
        credit_error: String,
        source: io::Error,
    },
    ReadKernelRandom {
        source: io::Error,
    },
    WriteSeed {
        path: PathBuf,
        source: io::Error,
    },
}

pub fn restore_linux_random_seed() -> Result<LinuxRandomSeedRestoreStatus, LinuxRandomSeedError> {
    let mut syscalls = LinuxRandomSeedSyscalls;
    restore_linux_random_seed_with_syscalls(
        Path::new(DEFAULT_RANDOM_SEED_PATH),
        Path::new(DEFAULT_RANDOM_DEVICE_PATH),
        &mut syscalls,
    )
}

pub fn save_linux_random_seed() -> Result<(), LinuxRandomSeedError> {
    let mut syscalls = LinuxRandomSeedSyscalls;
    save_linux_random_seed_with_syscalls(Path::new(DEFAULT_RANDOM_SEED_PATH), &mut syscalls)
}

fn restore_linux_random_seed_with_syscalls<S>(
    seed_path: &Path,
    random_device_path: &Path,
    syscalls: &mut S,
) -> Result<LinuxRandomSeedRestoreStatus, LinuxRandomSeedError>
where
    S: LinuxRandomSeedSyscallApi + ?Sized,
{
    let Some(seed) = syscalls
        .read_seed(seed_path, MAX_RESTORE_SEED_BYTES + 1)
        .map_err(|source| LinuxRandomSeedError::ReadSeed {
            path: seed_path.to_path_buf(),
            source,
        })?
    else {
        return Ok(LinuxRandomSeedRestoreStatus::Missing);
    };
    validate_seed_size(seed_path, seed.len())?;

    match syscalls.credit_seed(random_device_path, &seed) {
        Ok(()) => Ok(LinuxRandomSeedRestoreStatus::Credited),
        Err(error) => {
            let credit_error = error.to_string();
            syscalls
                .mix_seed(random_device_path, &seed)
                .map_err(|source| LinuxRandomSeedError::MixSeed {
                    random_device_path: random_device_path.to_path_buf(),
                    credit_error: credit_error.clone(),
                    source,
                })?;
            Ok(LinuxRandomSeedRestoreStatus::MixedWithoutCredit { credit_error })
        }
    }
}

fn save_linux_random_seed_with_syscalls<S>(
    seed_path: &Path,
    syscalls: &mut S,
) -> Result<(), LinuxRandomSeedError>
where
    S: LinuxRandomSeedSyscallApi + ?Sized,
{
    let mut seed = [0_u8; RANDOM_SEED_BYTES];
    syscalls
        .read_kernel_random(&mut seed)
        .map_err(|source| LinuxRandomSeedError::ReadKernelRandom { source })?;
    syscalls
        .write_seed_atomically(seed_path, &seed)
        .map_err(|source| LinuxRandomSeedError::WriteSeed {
            path: seed_path.to_path_buf(),
            source,
        })
}

fn validate_seed_size(seed_path: &Path, bytes: usize) -> Result<(), LinuxRandomSeedError> {
    if bytes == 0 || bytes > MAX_RESTORE_SEED_BYTES {
        Err(LinuxRandomSeedError::InvalidSeedSize {
            path: seed_path.to_path_buf(),
            bytes,
            max: MAX_RESTORE_SEED_BYTES,
        })
    } else {
        Ok(())
    }
}

trait LinuxRandomSeedSyscallApi {
    fn read_seed(&mut self, path: &Path, limit: usize) -> io::Result<Option<Vec<u8>>>;
    fn credit_seed(&mut self, random_device_path: &Path, seed: &[u8]) -> io::Result<()>;
    fn mix_seed(&mut self, random_device_path: &Path, seed: &[u8]) -> io::Result<()>;
    fn read_kernel_random(&mut self, out: &mut [u8]) -> io::Result<()>;
    fn write_seed_atomically(&mut self, path: &Path, seed: &[u8]) -> io::Result<()>;
}

#[derive(Debug, Clone, Copy, Default)]
struct LinuxRandomSeedSyscalls;

impl LinuxRandomSeedSyscallApi for LinuxRandomSeedSyscalls {
    fn read_seed(&mut self, path: &Path, limit: usize) -> io::Result<Option<Vec<u8>>> {
        let file = match open_read_file(path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error),
        };
        read_fd_with_limit(file.as_raw_fd(), limit).map(Some)
    }

    fn credit_seed(&mut self, random_device_path: &Path, seed: &[u8]) -> io::Result<()> {
        let file = open_random_device(random_device_path, RandomDeviceAccess::ReadWrite)?;
        add_entropy_fd(file.as_raw_fd(), seed)
    }

    fn mix_seed(&mut self, random_device_path: &Path, seed: &[u8]) -> io::Result<()> {
        let file = open_random_device(random_device_path, RandomDeviceAccess::Write)?;
        write_all_fd(file.as_raw_fd(), seed)
    }

    fn read_kernel_random(&mut self, out: &mut [u8]) -> io::Result<()> {
        getrandom_fill(out)
    }

    fn write_seed_atomically(&mut self, path: &Path, seed: &[u8]) -> io::Result<()> {
        write_seed_atomically(path, seed)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RandomDeviceAccess {
    ReadWrite,
    Write,
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

#[cfg(feature = "peios-boundary")]
fn open_random_device(path: &Path, access: RandomDeviceAccess) -> io::Result<peios::file::File> {
    let desired_access = match access {
        RandomDeviceAccess::ReadWrite => FileAccess::READ_DATA | FileAccess::WRITE_DATA,
        RandomDeviceAccess::Write => FileAccess::WRITE_DATA,
    };
    OpenOptions::new()
        .desired_access(desired_access)
        .open(None, path)
        .map_err(io::Error::from)
}

#[cfg(not(feature = "peios-boundary"))]
fn open_random_device(path: &Path, access: RandomDeviceAccess) -> io::Result<std::fs::File> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true);
    if access == RandomDeviceAccess::ReadWrite {
        options.read(true);
    }
    options.open(path)
}

fn add_entropy_fd(fd: i32, seed: &[u8]) -> io::Result<()> {
    let mut payload = rand_pool_payload(seed);
    let result = unsafe { libc::ioctl(fd, RNDADDENTROPY, payload.as_mut_ptr()) };
    if result < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

fn rand_pool_payload(seed: &[u8]) -> Vec<u32> {
    let words = seed.len().div_ceil(std::mem::size_of::<u32>());
    let mut payload = vec![0_u32; 2 + words];
    payload[0] = (seed.len() * 8) as u32;
    payload[1] = seed.len() as u32;
    let seed_window = unsafe {
        std::slice::from_raw_parts_mut(payload[2..].as_mut_ptr().cast::<u8>(), words * 4)
    };
    seed_window[..seed.len()].copy_from_slice(seed);
    payload
}

const fn rndaddentropy_request() -> libc::c_ulong {
    #[cfg(any(
        target_arch = "mips",
        target_arch = "mips64",
        target_arch = "powerpc",
        target_arch = "powerpc64"
    ))]
    {
        0x8008_5203
    }

    #[cfg(not(any(
        target_arch = "mips",
        target_arch = "mips64",
        target_arch = "powerpc",
        target_arch = "powerpc64"
    )))]
    {
        0x4008_5203
    }
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
    let mut buffer = [0_u8; 512];
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

fn write_seed_atomically(path: &Path, seed: &[u8]) -> io::Result<()> {
    let parent = path.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("seed path has no parent: {}", path.display()),
        )
    })?;
    std::fs::create_dir_all(parent)?;
    apply_seed_directory_sd(parent)?;

    let temp_path = temp_seed_path(path);
    let result = write_seed_temp_and_rename(path, &temp_path, parent, seed);
    if result.is_err() {
        let _ = std::fs::remove_file(&temp_path);
    }
    result
}

fn write_seed_temp_and_rename(
    path: &Path,
    temp_path: &Path,
    parent: &Path,
    seed: &[u8],
) -> io::Result<()> {
    let file = create_seed_temp_file(temp_path)?;
    write_all_fd(file.as_raw_fd(), seed)?;
    fsync_fd(file.as_raw_fd())?;
    drop(file);
    std::fs::rename(temp_path, path)?;
    fsync_directory(parent)
}

fn temp_seed_path(path: &Path) -> PathBuf {
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("random-seed");
    path.with_file_name(format!(".{file_name}.tmp.{}", std::process::id()))
}

#[cfg(feature = "peios-boundary")]
fn create_seed_temp_file(path: &Path) -> io::Result<peios::file::File> {
    let sd = random_seed_file_sd()?;
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
fn create_seed_temp_file(path: &Path) -> io::Result<std::fs::File> {
    std::fs::OpenOptions::new()
        .create(true)
        .truncate(true)
        .read(true)
        .write(true)
        .open(path)
}

#[cfg(feature = "peios-boundary")]
fn apply_seed_directory_sd(path: &Path) -> io::Result<()> {
    let sd = random_seed_dir_sd()?;
    peios::file::set_sd(
        None,
        path,
        SecInfo::OWNER | SecInfo::GROUP | SecInfo::DACL,
        &sd,
        0,
    )
    .map_err(io::Error::from)
}

#[cfg(not(feature = "peios-boundary"))]
fn apply_seed_directory_sd(_path: &Path) -> io::Result<()> {
    Ok(())
}

#[cfg(feature = "peios-boundary")]
fn random_seed_dir_sd() -> io::Result<peios::security::SecurityDescriptor> {
    peios::security::sddl::parse(RANDOM_SEED_DIR_SDDL).map_err(io::Error::from)
}

#[cfg(feature = "peios-boundary")]
fn random_seed_file_sd() -> io::Result<peios::security::SecurityDescriptor> {
    peios::security::sddl::parse(RANDOM_SEED_FILE_SDDL).map_err(io::Error::from)
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
        .options(peios::file::CreateOptions::DIRECTORY)
        .open(None, path)
        .map_err(io::Error::from)
}

#[cfg(not(feature = "peios-boundary"))]
fn open_directory_for_fsync(path: &Path) -> io::Result<std::fs::File> {
    std::fs::File::open(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_seed_is_a_clean_noop() {
        let mut syscalls = FakeRandomSeedSyscalls::default();

        let status = restore_linux_random_seed_with_syscalls(
            Path::new("/seed"),
            Path::new("/dev/urandom"),
            &mut syscalls,
        )
        .expect("restore");

        assert_eq!(status, LinuxRandomSeedRestoreStatus::Missing);
        assert!(syscalls.calls.is_empty());
    }

    #[test]
    fn present_seed_is_credited_to_kernel_rng() {
        let mut syscalls = FakeRandomSeedSyscalls {
            seed: Some(vec![1, 2, 3, 4]),
            ..FakeRandomSeedSyscalls::default()
        };

        let status = restore_linux_random_seed_with_syscalls(
            Path::new("/seed"),
            Path::new("/dev/urandom"),
            &mut syscalls,
        )
        .expect("restore");

        assert_eq!(status, LinuxRandomSeedRestoreStatus::Credited);
        assert_eq!(
            syscalls.calls,
            vec![FakeRandomSeedCall::Credit {
                path: "/dev/urandom".into(),
                bytes: vec![1, 2, 3, 4],
            }],
        );
    }

    #[test]
    fn credit_failure_mixes_without_credit_and_reports_warning_status() {
        let mut syscalls = FakeRandomSeedSyscalls {
            seed: Some(vec![9, 8, 7]),
            credit_error: Some(io::Error::from(io::ErrorKind::PermissionDenied)),
            ..FakeRandomSeedSyscalls::default()
        };

        let status = restore_linux_random_seed_with_syscalls(
            Path::new("/seed"),
            Path::new("/dev/urandom"),
            &mut syscalls,
        )
        .expect("restore");

        assert_eq!(
            status,
            LinuxRandomSeedRestoreStatus::MixedWithoutCredit {
                credit_error: "permission denied".to_string(),
            },
        );
        assert_eq!(
            syscalls.calls,
            vec![
                FakeRandomSeedCall::Credit {
                    path: "/dev/urandom".into(),
                    bytes: vec![9, 8, 7],
                },
                FakeRandomSeedCall::Mix {
                    path: "/dev/urandom".into(),
                    bytes: vec![9, 8, 7],
                },
            ],
        );
    }

    #[test]
    fn save_reads_fresh_kernel_random_and_writes_it_atomically() {
        let mut syscalls = FakeRandomSeedSyscalls {
            kernel_random_byte: 0xaa,
            ..FakeRandomSeedSyscalls::default()
        };

        save_linux_random_seed_with_syscalls(Path::new("/seed"), &mut syscalls).expect("save");

        assert_eq!(
            syscalls.calls,
            vec![FakeRandomSeedCall::WriteSeed {
                path: "/seed".into(),
                bytes: vec![0xaa; RANDOM_SEED_BYTES],
            }],
        );
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    enum FakeRandomSeedCall {
        Credit { path: String, bytes: Vec<u8> },
        Mix { path: String, bytes: Vec<u8> },
        WriteSeed { path: String, bytes: Vec<u8> },
    }

    #[derive(Debug, Default)]
    struct FakeRandomSeedSyscalls {
        seed: Option<Vec<u8>>,
        credit_error: Option<io::Error>,
        kernel_random_byte: u8,
        calls: Vec<FakeRandomSeedCall>,
    }

    impl LinuxRandomSeedSyscallApi for FakeRandomSeedSyscalls {
        fn read_seed(&mut self, _path: &Path, _limit: usize) -> io::Result<Option<Vec<u8>>> {
            Ok(self.seed.clone())
        }

        fn credit_seed(&mut self, random_device_path: &Path, seed: &[u8]) -> io::Result<()> {
            self.calls.push(FakeRandomSeedCall::Credit {
                path: random_device_path.display().to_string(),
                bytes: seed.to_vec(),
            });
            match self.credit_error.take() {
                Some(error) => Err(error),
                None => Ok(()),
            }
        }

        fn mix_seed(&mut self, random_device_path: &Path, seed: &[u8]) -> io::Result<()> {
            self.calls.push(FakeRandomSeedCall::Mix {
                path: random_device_path.display().to_string(),
                bytes: seed.to_vec(),
            });
            Ok(())
        }

        fn read_kernel_random(&mut self, out: &mut [u8]) -> io::Result<()> {
            out.fill(self.kernel_random_byte);
            Ok(())
        }

        fn write_seed_atomically(&mut self, path: &Path, seed: &[u8]) -> io::Result<()> {
            self.calls.push(FakeRandomSeedCall::WriteSeed {
                path: path.display().to_string(),
                bytes: seed.to_vec(),
            });
            Ok(())
        }
    }
}
