use crate::runtime::{RuntimeEventRegistrar, RuntimeEventRegistrationError, RuntimeEventSource};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RegistrarCall {
    pub(crate) fd: i32,
    pub(crate) source: RuntimeEventSource,
}

#[derive(Debug, Default)]
pub(crate) struct FakeRegistrar {
    pub(crate) calls: Vec<RegistrarCall>,
    pub(crate) unregister_calls: Vec<i32>,
    fail_register: bool,
}

impl FakeRegistrar {
    pub(crate) fn failing_registration() -> Self {
        Self {
            fail_register: true,
            ..Self::default()
        }
    }
}

impl RuntimeEventRegistrar for FakeRegistrar {
    fn register_source(
        &mut self,
        fd: i32,
        source: RuntimeEventSource,
    ) -> Result<(), RuntimeEventRegistrationError> {
        self.calls.push(RegistrarCall { fd, source });
        if self.fail_register {
            Err(RuntimeEventRegistrationError::Register {
                fd,
                source,
                message: "register failed".to_string(),
            })
        } else {
            Ok(())
        }
    }

    fn unregister_source(&mut self, fd: i32) -> Result<(), RuntimeEventRegistrationError> {
        self.unregister_calls.push(fd);
        Ok(())
    }
}
