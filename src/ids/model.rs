use std::fmt;
use std::str::FromStr;

use super::uuid_v7::{UuidV7, UuidV7ParseError, parse_uuid_v7_canonical};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct OperationId(pub(super) UuidV7);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct JobId(pub(super) UuidV7);

impl OperationId {
    pub fn as_bytes(self) -> [u8; 16] {
        self.0.as_bytes()
    }

    pub fn to_canonical_string(self) -> String {
        self.0.to_canonical_string()
    }

    pub fn parse_canonical_str(value: &str) -> Result<Self, OperationIdParseError> {
        parse_uuid_v7_canonical(value)
            .map(Self)
            .map_err(OperationIdParseError::Uuid)
    }
}

impl JobId {
    pub fn as_bytes(self) -> [u8; 16] {
        self.0.as_bytes()
    }

    pub fn to_canonical_string(self) -> String {
        self.0.to_canonical_string()
    }
}

impl fmt::Display for OperationId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_canonical_string())
    }
}

impl fmt::Display for JobId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_canonical_string())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperationIdParseError {
    Uuid(UuidV7ParseError),
}

impl FromStr for OperationId {
    type Err = OperationIdParseError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::parse_canonical_str(value)
    }
}
