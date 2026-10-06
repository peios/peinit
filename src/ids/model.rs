use std::fmt;
use std::str::FromStr;

use super::uuid_v7::{UuidV7, UuidV7ParseError, parse_uuid_v7_canonical};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct OperationId(pub(super) UuidV7);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct JobId(pub(super) UuidV7);

/// The PCDS binary form of a UUID's sixteen RFC 9562 bytes: the first three
/// fields little-endian, the last eight bytes as they are. It is the form
/// a `bin.guid` event field carries, so that the GUID reads back as the
/// same canonical text (PCDS §2, PGSS §6.5).
fn pcds_guid_bytes(rfc: [u8; 16]) -> [u8; 16] {
    let mut out = rfc;
    out[0..4].reverse();
    out[4..6].reverse();
    out[6..8].reverse();
    out
}

impl OperationId {
    pub fn as_bytes(self) -> [u8; 16] {
        self.0.as_bytes()
    }

    /// The operation as a PCDS binary GUID, for a `bin.guid` field.
    pub fn as_guid_bytes(self) -> [u8; 16] {
        pcds_guid_bytes(self.as_bytes())
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

    /// The job as a PCDS binary GUID, for a `bin.guid` field or an audit
    /// context.
    pub fn as_guid_bytes(self) -> [u8; 16] {
        pcds_guid_bytes(self.as_bytes())
    }

    pub fn to_canonical_string(self) -> String {
        self.0.to_canonical_string()
    }

    pub fn parse_canonical_str(value: &str) -> Result<Self, JobIdParseError> {
        parse_uuid_v7_canonical(value)
            .map(Self)
            .map_err(JobIdParseError::Uuid)
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobIdParseError {
    Uuid(UuidV7ParseError),
}

impl FromStr for JobId {
    type Err = JobIdParseError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::parse_canonical_str(value)
    }
}
