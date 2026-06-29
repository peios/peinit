use std::io;

use super::loopback::{LoopbackNetlink, bring_up_loopback};

#[test]
fn loopback_success_returns_ok() {
    let mut netlink = FakeLoopbackNetlink::ok();

    bring_up_loopback(&mut netlink).expect("loopback up");

    assert_eq!(netlink.calls, 1);
}

#[test]
fn loopback_failure_is_reported_to_phase1_infrastructure() {
    let mut netlink = FakeLoopbackNetlink::fail(libc::EPERM);

    let error = bring_up_loopback(&mut netlink).expect_err("loopback failure");

    assert_eq!(error.raw_os_error(), Some(libc::EPERM));
    assert_eq!(netlink.calls, 1);
}

struct FakeLoopbackNetlink {
    calls: usize,
    result: io::Result<()>,
}

impl FakeLoopbackNetlink {
    fn ok() -> Self {
        Self {
            calls: 0,
            result: Ok(()),
        }
    }

    fn fail(errno: i32) -> Self {
        Self {
            calls: 0,
            result: Err(io::Error::from_raw_os_error(errno)),
        }
    }
}

impl LoopbackNetlink for FakeLoopbackNetlink {
    fn bring_up_loopback(&mut self) -> io::Result<()> {
        self.calls += 1;
        self.result
            .as_ref()
            .map(|_| ())
            .map_err(|error| io::Error::from_raw_os_error(error.raw_os_error().unwrap()))
    }
}
