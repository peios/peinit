use super::name::RegistryNameDecodeError;
use crate::registry::ServiceRegistryDecodeError;

#[derive(Debug)]
pub enum LcsRegistryReadError {
    OpenRoot(peios::Error),
    ReadServicesRoot(peios::Error),
    DecodeServicesRoot(ServiceRegistryDecodeError),
    EnumerateService(peios::Error),
    Name(RegistryNameDecodeError),
    OpenService {
        service: String,
        source: peios::Error,
    },
    ReadValues {
        service: String,
        source: peios::Error,
    },
    DecodeService {
        service: String,
        source: ServiceRegistryDecodeError,
    },
    OpenBoot(peios::Error),
    ReadBoot(peios::Error),
    DecodeBoot(ServiceRegistryDecodeError),
    OpenInit(peios::Error),
    ReadInit(peios::Error),
    DecodeInit(ServiceRegistryDecodeError),
    OpenProvisionedPaths(peios::Error),
    EnumerateProvisionedPath(peios::Error),
    OpenProvisionedPath {
        entry: String,
        source: peios::Error,
    },
    ReadProvisionedPath {
        entry: String,
        source: peios::Error,
    },
    OpenGlobalEnvironment(peios::Error),
    ReadGlobalEnvironment(peios::Error),
    DecodeGlobalEnvironment(ServiceRegistryDecodeError),
    OpenEventd(peios::Error),
    ReadEventd(peios::Error),
    OpenServicesSchema(peios::Error),
    ReadServicesSchema(peios::Error),
    InvalidServicesSchemaType(u32),
    InvalidServicesSchemaLength {
        actual_len: usize,
    },
    Provision {
        stage: &'static str,
        source: peios::Error,
    },
}
