#[cfg_attr(not(any(test, feature = "peios-boundary")), allow(dead_code))]
pub const EV_KEY: u16 = 0x01;
#[cfg_attr(not(any(test, feature = "peios-boundary")), allow(dead_code))]
pub const KEY_POWER: u16 = 116;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinuxPowerButtonRead {
    Pressed,
    Ignored,
    WouldBlock,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinuxPowerButtonReadError {
    StaleFd {
        fd: i32,
    },
    Read {
        fd: i32,
        message: String,
    },
    ShortRead {
        fd: i32,
        bytes: usize,
        expected: usize,
    },
}
