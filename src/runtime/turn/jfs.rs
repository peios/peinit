use super::{
    RuntimeEventRegistrar, RuntimeJfsDeviceTurn, RuntimeShutdownEventTurn,
    RuntimeShutdownEventTurnError,
};

pub(super) fn process_jfs_device_event<R>(
    fd: i32,
    registrar: &mut R,
) -> Result<RuntimeShutdownEventTurn, RuntimeShutdownEventTurnError>
where
    R: RuntimeEventRegistrar + ?Sized,
{
    // PSD-007 intentionally leaves the byte-level /dev/jfs ABI to the JFS
    // subsystem. Until that boundary is specified, peinit stops at the parse
    // handoff and disables the fd to avoid spinning on an unreadable protocol.
    registrar
        .unregister_source(fd)
        .map_err(RuntimeShutdownEventTurnError::JfsRegistration)?;
    Ok(RuntimeShutdownEventTurn::JfsDevice {
        turn: RuntimeJfsDeviceTurn::ParseBoundaryReached {
            fd,
            source_disabled_until_abi_exists: true,
        },
    })
}
