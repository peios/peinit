use crate::boundary::LinuxEpollEvent;
use crate::runtime::RuntimeEventSource;

use super::{RuntimeEventWaitError, model::decode_runtime_epoll_events};

#[test]
fn epoll_events_decode_to_runtime_sources() {
    let events = vec![
        LinuxEpollEvent::read(RuntimeEventSource::Pid1Signal.token()),
        LinuxEpollEvent::read(RuntimeEventSource::ControlConnection { fd: 42 }.token()),
        LinuxEpollEvent::read(RuntimeEventSource::ShutdownDeadlineTimer.token()),
        LinuxEpollEvent::read(RuntimeEventSource::NotifySocket.token()),
        LinuxEpollEvent::read(RuntimeEventSource::LifecycleDeadlineTimer.token()),
        LinuxEpollEvent::read(RuntimeEventSource::JfsDevice { fd: 44 }.token()),
        LinuxEpollEvent::read(RuntimeEventSource::FilesystemCheckHelper { result_fd: 45 }.token()),
        LinuxEpollEvent::read(RuntimeEventSource::FilesystemCheckHelperExit { pidfd: 46 }.token()),
    ];

    assert_eq!(
        decode_runtime_epoll_events(events).expect("decode"),
        vec![
            RuntimeEventSource::Pid1Signal,
            RuntimeEventSource::ControlConnection { fd: 42 },
            RuntimeEventSource::ShutdownDeadlineTimer,
            RuntimeEventSource::NotifySocket,
            RuntimeEventSource::LifecycleDeadlineTimer,
            RuntimeEventSource::JfsDevice { fd: 44 },
            RuntimeEventSource::FilesystemCheckHelper { result_fd: 45 },
            RuntimeEventSource::FilesystemCheckHelperExit { pidfd: 46 },
        ],
    );
}

#[test]
fn epoll_decode_error_keeps_original_event() {
    let event = LinuxEpollEvent::read(0xFFFF);
    let err = decode_runtime_epoll_events(vec![event]).expect_err("decode error");

    assert!(matches!(
        err,
        RuntimeEventWaitError::Decode {
            event: returned,
            ..
        } if returned == event
    ));
}
