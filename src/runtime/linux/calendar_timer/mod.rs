mod entry;
mod error;
mod event;
mod random;
#[cfg(feature = "peios-registry")]
mod registration;
mod table;
#[cfg(test)]
mod test_support;

#[cfg(feature = "peios-registry")]
mod boot;
#[cfg(feature = "peios-registry")]
mod reload;

#[cfg(feature = "peios-registry")]
pub(super) use self::boot::LinuxCalendarTimerBootRegistration;
#[cfg(feature = "peios-registry")]
pub(crate) use self::error::LinuxCalendarTimerError;
pub(super) use self::table::LinuxCalendarTimerTable;
