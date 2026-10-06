//! Building an event payload as the nested map PGSS §6.4 describes.
//!
//! A field is named by its dotted catalogue path (`object.service.name`);
//! [`Payload`] turns the paths it is given into nested string-keyed maps,
//! in the order they were first named. A field with no value is not set at
//! all — absence is an absent key, never nil (PGSS §6.5) — which is what
//! the `set_opt` family is for.

use std::str::FromStr;

use peios::msgpack::{self, Writer};

use crate::boundary::{BoundaryError, KmesEvent};
use crate::ids::{JobId, OperationId};
use crate::security::TokenSummary;

/// One value, in a PGSS §6.5 wire form.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Value {
    Str(String),
    Uint(u64),
    Int(i64),
    Bool(bool),
    Bin(Vec<u8>),
    StrArray(Vec<String>),
    BinArray(Vec<Vec<u8>>),
}

impl From<&str> for Value {
    fn from(value: &str) -> Self {
        Self::Str(value.to_string())
    }
}

impl From<&String> for Value {
    fn from(value: &String) -> Self {
        Self::Str(value.clone())
    }
}

impl From<String> for Value {
    fn from(value: String) -> Self {
        Self::Str(value)
    }
}

impl From<u64> for Value {
    fn from(value: u64) -> Self {
        Self::Uint(value)
    }
}

impl From<u32> for Value {
    fn from(value: u32) -> Self {
        Self::Uint(u64::from(value))
    }
}

impl From<i32> for Value {
    fn from(value: i32) -> Self {
        Self::Int(i64::from(value))
    }
}

impl From<i64> for Value {
    fn from(value: i64) -> Self {
        Self::Int(value)
    }
}

impl From<bool> for Value {
    fn from(value: bool) -> Self {
        Self::Bool(value)
    }
}

impl From<&[String]> for Value {
    fn from(value: &[String]) -> Self {
        Self::StrArray(value.to_vec())
    }
}

impl From<JobId> for Value {
    /// `bin.guid`: the job's sixteen UUID bytes.
    fn from(value: JobId) -> Self {
        Self::Bin(value.as_bytes().to_vec())
    }
}

impl From<OperationId> for Value {
    /// `bin.guid`: the operation's sixteen UUID bytes.
    fn from(value: OperationId) -> Self {
        Self::Bin(value.as_bytes().to_vec())
    }
}

#[derive(Debug, Clone)]
enum Node {
    Value(Value),
    Map(Vec<(&'static str, Node)>),
}

/// An event payload under construction.
#[derive(Debug, Clone, Default)]
pub(crate) struct Payload {
    root: Vec<(&'static str, Node)>,
}

impl Payload {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Set the field at the dotted `path`.
    ///
    /// Setting a path twice replaces the value. A path that is a value in
    /// one place and a map in another is a peinit bug (PGSS §6.4 forbids
    /// it), and panics in a debug build.
    pub(crate) fn set(&mut self, path: &'static str, value: impl Into<Value>) -> &mut Self {
        let mut level = &mut self.root;
        let mut segments = path.split('.').peekable();
        while let Some(segment) = segments.next() {
            let last = segments.peek().is_none();
            let position = level.iter().position(|(key, _)| *key == segment);
            if last {
                let node = Node::Value(value.into());
                match position {
                    Some(index) => {
                        debug_assert!(
                            matches!(level[index].1, Node::Value(_)),
                            "{path} is a map elsewhere in this payload",
                        );
                        level[index].1 = node;
                    }
                    None => level.push((segment, node)),
                }
                return self;
            }
            let index = match position {
                Some(index) => index,
                None => {
                    level.push((segment, Node::Map(Vec::new())));
                    level.len() - 1
                }
            };
            if !matches!(level[index].1, Node::Map(_)) {
                debug_assert!(false, "{path} passes through a value in this payload");
                level[index].1 = Node::Map(Vec::new());
            }
            let Node::Map(children) = &mut level[index].1 else {
                unreachable!("made a map just above");
            };
            level = children;
        }
        self
    }

    /// Set the field when there is a value, and leave it absent otherwise.
    pub(crate) fn set_opt<V: Into<Value>>(
        &mut self,
        path: &'static str,
        value: Option<V>,
    ) -> &mut Self {
        if let Some(value) = value {
            self.set(path, value);
        }
        self
    }

    /// Set a `bin.sid` field from the SDDL string peinit holds. Text that is
    /// not a SID has no binary form, and leaves the field absent rather than
    /// costing the whole record.
    pub(crate) fn set_sid(&mut self, path: &'static str, sddl: &str) -> &mut Self {
        if let Some(bytes) = sid_bytes(sddl) {
            self.set(path, Value::Bin(bytes));
        }
        self
    }

    /// The token fields of a participant (`subject.token` or
    /// `object.job.token`): its user and groups as `bin.sid`, its present
    /// and enabled privileges as `uint.flags`. The token's names are never
    /// carried (PGSS §6.5).
    ///
    /// A summary with no groups is one peinit made from the user SID alone,
    /// as it does for a control-channel caller; it says nothing about the
    /// token's groups or privileges, so only the user is written. A real
    /// token always has groups.
    pub(crate) fn set_token(&mut self, paths: TokenPaths, token: &TokenSummary) -> &mut Self {
        self.set_sid(paths.sid, token.caller_sid());
        if token.group_sids.is_empty() {
            return self;
        }
        let groups = token
            .group_sids
            .iter()
            .filter_map(|sid| sid_bytes(sid))
            .collect::<Vec<_>>();
        self.set(paths.groups, Value::BinArray(groups));
        self.set(paths.privileges, privilege_flags(&token.present_privileges));
        self.set(
            paths.privileges_enabled,
            privilege_flags(&token.enabled_privileges),
        );
        self
    }

    /// Encode the payload as `event_type`.
    pub(crate) fn finish(&self, event_type: &'static str) -> Result<KmesEvent, BoundaryError> {
        let mut writer = Writer::new();
        write_map(&mut writer, &self.root);
        let payload = writer
            .to_bytes()
            .map_err(|error| BoundaryError::Kmes(error.to_string()))?;
        msgpack::validate(&payload, msgpack::DEFAULT_MAX_DEPTH)
            .map_err(|error| BoundaryError::Kmes(error.to_string()))?;
        Ok(KmesEvent::new(event_type, payload))
    }
}

/// Where a participant's token fields go.
#[derive(Debug, Clone, Copy)]
pub(crate) struct TokenPaths {
    pub sid: &'static str,
    pub groups: &'static str,
    pub privileges: &'static str,
    pub privileges_enabled: &'static str,
}

pub(crate) const SUBJECT_TOKEN: TokenPaths = TokenPaths {
    sid: "subject.token.sid",
    groups: "subject.token.groups",
    privileges: "subject.token.privileges",
    privileges_enabled: "subject.token.privileges-enabled",
};

pub(crate) const JOB_TOKEN: TokenPaths = TokenPaths {
    sid: "object.job.token.sid",
    groups: "object.job.token.groups",
    privileges: "object.job.token.privileges",
    privileges_enabled: "object.job.token.privileges-enabled",
};

fn write_map(writer: &mut Writer, entries: &[(&'static str, Node)]) {
    writer.write_map(u32::try_from(entries.len()).unwrap_or(u32::MAX));
    for (key, node) in entries {
        writer.write_str(key);
        match node {
            Node::Map(children) => write_map(writer, children),
            Node::Value(value) => write_value(writer, value),
        }
    }
}

fn write_value(writer: &mut Writer, value: &Value) {
    match value {
        Value::Str(value) => {
            writer.write_str(value);
        }
        Value::Uint(value) => {
            writer.write_uint(*value);
        }
        Value::Int(value) => {
            writer.write_int(*value);
        }
        Value::Bool(value) => {
            writer.write_bool(*value);
        }
        Value::Bin(value) => {
            writer.write_bin(value);
        }
        Value::StrArray(values) => {
            writer.write_array(u32::try_from(values.len()).unwrap_or(u32::MAX));
            for value in values {
                writer.write_str(value);
            }
        }
        Value::BinArray(values) => {
            writer.write_array(u32::try_from(values.len()).unwrap_or(u32::MAX));
            for value in values {
                writer.write_bin(value);
            }
        }
    }
}

/// A SID's binary form, from the SDDL string peinit holds it as.
pub(crate) fn sid_bytes(sddl: &str) -> Option<Vec<u8>> {
    peios::security::Sid::from_str(sddl)
        .ok()
        .map(|sid| sid.as_bytes().to_vec())
}

/// The `KACS_SE_*_PRIVILEGE` flags of a list of privilege names. A name the
/// SDK does not know has no bit, and is left out.
pub(crate) fn privilege_flags(names: &[String]) -> u64 {
    names
        .iter()
        .filter_map(|name| peios::security::Privileges::parse_name(name))
        .fold(0, |flags, privilege| flags | privilege.bits())
}
