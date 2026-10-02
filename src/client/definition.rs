//! A service's definition as the values of its registry key, for a program
//! that shows or changes it: what each field holds (`FIELDS`), the values as
//! a person states them, and whether peinit would take them (`check`).
//!
//! There is no registry here. The program reads the key's values into a
//! [`Definition`], changes it, checks it, and writes back what [`changes`]
//! says, as it can: the values it does not know of are kept as they are,
//! and only what was changed is written, so nothing it did not touch is
//! written over.
//!
//! Passing `check` is passing peinit's own decoder, with each `timer:`
//! trigger's schedule parsed as peinit parses it when the timer arms. It is
//! not all peinit checks: privilege names are checked when the token is
//! made, the services a definition names when the graph is built, and the
//! identity by authd. What peinit says of the service once the change is
//! read (`status`) is the final word.

use crate::registry::{
    FieldInfo, FieldKind, RawRegistryValue, RegistryValueType, SERVICE_FIELD_INFO,
    ServiceRegistryDecodeError, build_service_definition_from_registry_values, service_field,
};
use crate::service::ServiceTrigger;

/// Every field of a service definition.
pub const FIELDS: &[FieldInfo] = SERVICE_FIELD_INFO;

/// A service's definition, as the values of its key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Definition {
    pub name: String,
    pub values: Vec<RawRegistryValue>,
}

/// One change to a definition's key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    /// The value, set to this.
    Set(RawRegistryValue),
    /// The value of this name, taken away.
    Unset(String),
}

/// Why peinit would not take a definition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    /// The field to show it beside, where it is about one. A missing
    /// `ImagePath` is shown beside `ImagePath`, though there is no value.
    pub field: Option<&'static str>,
    /// What is wrong, in words.
    pub message: String,
}

impl std::fmt::Display for Problem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl Definition {
    pub fn new(name: &str, values: Vec<RawRegistryValue>) -> Definition {
        Definition { name: name.into(), values }
    }

    /// The value of `field`, by its name in any case.
    pub fn get(&self, field: &str) -> Option<&RawRegistryValue> {
        self.values.iter().find(|value| value.name.eq_ignore_ascii_case(field))
    }

    /// The value of `field` as a person states it: a list a line an item, a
    /// choice by its name, yes or no. `None` where there is none. A value
    /// not of the field's type is stated as what it holds, and `check` says
    /// what is wrong with it.
    pub fn text(&self, field: &str) -> Option<String> {
        let value = self.get(field)?;
        let kind = service_field(field).map(|info| info.kind);
        Some(match (value.value_type, kind) {
            (RegistryValueType::Dword, Some(FieldKind::YesNo)) => match dword(&value.data) {
                Some(0) => "no".into(),
                Some(1) => "yes".into(),
                Some(other) => other.to_string(),
                None => hex(&value.data),
            },
            (RegistryValueType::Dword, Some(FieldKind::Choice(names))) => match dword(&value.data) {
                Some(number) => names.iter().find(|(value, _)| *value == number).map_or_else(|| number.to_string(), |(_, name)| (*name).into()),
                None => hex(&value.data),
            },
            (RegistryValueType::Dword, _) => dword(&value.data).map_or_else(|| hex(&value.data), |number| number.to_string()),
            (RegistryValueType::Sz, _) => String::from_utf8_lossy(value.data.strip_suffix(&[0]).unwrap_or(&value.data)).into_owned(),
            (RegistryValueType::MultiSz, _) => strings(&value.data).join("\n"),
            _ => hex(&value.data),
        })
    }

    /// Sets `field` from what a person typed: `text` as `text` would state
    /// it, a list a line an item. Nothing but white space unsets it.
    pub fn set(&mut self, field: &str, text: &str) -> Result<(), String> {
        let Some(info) = service_field(field) else { return Err(format!("{field} is not a field of a service definition.")) };
        if text.trim().is_empty() {
            self.unset(info.name);
            return Ok(());
        }
        let data = match info.kind {
            FieldKind::Text => sz(text.trim()),
            FieldKind::List => multi_sz(&text.lines().map(str::trim).filter(|line| !line.is_empty()).map(String::from).collect::<Vec<_>>()),
            FieldKind::Number { .. } => {
                let number = text.trim().parse::<u32>().map_err(|_| format!("{} is a whole number, from 0 to {}.", info.name, u32::MAX))?;
                number.to_le_bytes().to_vec()
            }
            FieldKind::YesNo => match text.trim().to_ascii_lowercase().as_str() {
                "yes" | "1" | "true" | "on" => 1u32.to_le_bytes().to_vec(),
                "no" | "0" | "false" | "off" => 0u32.to_le_bytes().to_vec(),
                _ => return Err(format!("{} is yes or no.", info.name)),
            },
            FieldKind::Choice(names) => {
                let text = text.trim();
                let number = names
                    .iter()
                    .find(|(_, name)| name.eq_ignore_ascii_case(text))
                    .map(|(number, _)| *number)
                    .or_else(|| text.parse::<u32>().ok().filter(|number| names.iter().any(|(value, _)| value == number)));
                let Some(number) = number else {
                    let names: Vec<&str> = names.iter().map(|(_, name)| *name).collect();
                    return Err(format!("{} is one of {}.", info.name, names.join(", ")));
                };
                number.to_le_bytes().to_vec()
            }
            FieldKind::Binary => return Err(format!("{} is not set from text.", info.name)),
        };
        self.put(info, data);
        Ok(())
    }

    /// Sets the list `field` to `items`, each as it is, empty or not.
    pub fn set_list(&mut self, field: &str, items: &[String]) -> Result<(), String> {
        let Some(info) = service_field(field).filter(|info| info.kind == FieldKind::List) else { return Err(format!("{field} is not a list.")) };
        self.put(info, multi_sz(items));
        Ok(())
    }

    /// Takes the value of `field` away, whatever case its name is in.
    pub fn unset(&mut self, field: &str) {
        self.values.retain(|value| !value.name.eq_ignore_ascii_case(field));
    }

    /// Puts `data` in `info`'s value, keeping the name it has if it has one.
    fn put(&mut self, info: &FieldInfo, data: Vec<u8>) {
        let value_type = info.kind.value_type();
        match self.values.iter_mut().find(|value| value.name.eq_ignore_ascii_case(info.name)) {
            Some(value) => {
                value.value_type = value_type;
                value.data = data;
            }
            None => self.values.push(RawRegistryValue { name: info.name.into(), value_type, data }),
        }
    }

    /// The values that are not fields of a definition: kept as they are,
    /// and none of peinit's concern but `LastTimerRun`, which it writes.
    pub fn others(&self) -> impl Iterator<Item = &RawRegistryValue> {
        self.values.iter().filter(|value| service_field(&value.name).is_none())
    }

    /// Whether peinit's decoder would take it, and each `timer:` schedule.
    pub fn check(&self) -> Result<(), Problem> {
        let decoded = build_service_definition_from_registry_values(&self.name, &self.values).map_err(|error| problem(&error))?;
        for trigger in &decoded.triggers {
            if let ServiceTrigger::Timer { schedule } = trigger {
                crate::timer::calendar::parse_calendar_schedule(schedule).map_err(|error| Problem {
                    field: Some("Triggers"),
                    message: format!("“timer:{schedule}” is not a schedule: {error}."),
                })?;
            }
        }
        Ok(())
    }
}

/// What turns `from` into `to`: each value set or taken away, compared by
/// name in any case.
pub fn changes(from: &Definition, to: &Definition) -> Vec<Change> {
    let mut changes: Vec<Change> = to
        .values
        .iter()
        .filter(|value| from.get(&value.name) != Some(*value))
        .cloned()
        .map(Change::Set)
        .collect();
    changes.extend(from.values.iter().filter(|value| to.get(&value.name).is_none()).map(|value| Change::Unset(value.name.clone())));
    changes
}

/// A decode failure, in words, beside the field it is about.
pub fn problem(error: &ServiceRegistryDecodeError) -> Problem {
    use ServiceRegistryDecodeError as E;
    let quoted = |value: &str| format!("“{value}”");
    let (field, message) = match error {
        E::InvalidServiceName { service } => (
            Some("name"),
            format!("{} is not a service name: a name is 1 to 128 letters, digits, dots, dashes and underscores.", quoted(service)),
        ),
        E::MissingImagePath => (Some("ImagePath"), "It needs a program to run: ImagePath is not set.".into()),
        E::DuplicateField { field } => (Some(*field), format!("{field} is set twice, under names that differ only in case.")),
        E::TypeMismatch { field, expected, actual } => {
            (Some(*field), format!("{field} is {}, but holds {}.", type_words(*expected), type_words(*actual)))
        }
        E::MalformedString { field, .. } => (Some(*field), format!("{field} is not text the service manager can read.")),
        E::MalformedMultiString { field, .. } => (Some(*field), format!("{field} is not a list the service manager can read.")),
        E::MalformedDword { field, .. } => (Some(*field), format!("{field} is not a 32-bit number.")),
        E::UnknownDword { field, value } => {
            let names = match service_field(field).map(|info| info.kind) {
                Some(FieldKind::YesNo) => " It is yes or no.".to_string(),
                Some(FieldKind::Choice(names)) => format!(" It is one of {}.", names.iter().map(|(_, name)| *name).collect::<Vec<_>>().join(", ")),
                _ => String::new(),
            };
            (Some(*field), format!("{value} is not something {field} can be.{names}"))
        }
        E::InvalidSuccessExitCode { value } => (Some("SuccessExitCodes"), format!("{} is not an exit code: each is a number from 0 to 255.", quoted(value))),
        E::InvalidEnvironmentVariable { value } => (Some("Environment"), format!("{} is not a variable: each is NAME=value.", quoted(value))),
        E::InvalidListEntry { field, value } => (Some(*field), format!("{} cannot be one of {field}.", quoted(value))),
        E::InvalidServiceReference { field, value } => (
            Some(*field),
            format!("{} is not a service: each is a service's name, with a readiness level after a colon if it has one (netd:routed).", quoted(value)),
        ),
        E::InvalidTrigger { value } => (
            Some("Triggers"),
            format!("{} is not a trigger: boot, boot:settled, tty:released and timer: followed by a schedule are.", quoted(value)),
        ),
        E::InvalidAbsolutePath { field, value } => (Some(*field), format!("{field} is a path from the root, beginning with /, which {} is not.", quoted(value))),
        E::InvalidRuntimeDirectory { field, value } => (Some(*field), format!("{} is not a directory name to make under /run.", quoted(value))),
        E::InvalidExecutableCommand { field, value, source } => (
            Some(*field),
            match source {
                crate::execution::command::ExecutableCommandParseError::Empty => format!("{field} has a command with nothing in it."),
                crate::execution::command::ExecutableCommandParseError::UnclosedDoubleQuote => format!("{} has a double quote that is not closed.", quoted(value)),
                crate::execution::command::ExecutableCommandParseError::RelativeExecutable { executable } => {
                    format!("The program in {} is a path from the root, beginning with /, which {} is not.", quoted(value), quoted(executable))
                }
            },
        ),
        E::InvalidReloadSignal { value } => (Some("ExecReload"), format!("{} is not a signal: signal: is followed by a signal's name, as in signal:HUP.", quoted(value))),
        E::InvalidCheck { field, value } => (Some(*field), format!("{} is not a check {field} can make.", quoted(value))),
        E::NonCachedRegistryCheck { field, key } => (Some(*field), format!("{field} checks {key}, a registry key the service manager does not keep and so cannot check.")),
        E::FieldRequiresTtyPath { field } => (Some(*field), format!("{field} means something only with a terminal, and TTYPath is not set.")),
        E::InvalidProvisionedPathName { .. } | E::MissingProvisionedPathField { .. } | E::InvalidProvisionedPathKind { .. } => {
            (None, "It is not a service definition.".into())
        }
    };
    Problem { field, message }
}

fn type_words(value_type: RegistryValueType) -> String {
    match value_type {
        RegistryValueType::Sz => "text (REG_SZ)".into(),
        RegistryValueType::MultiSz => "a list (REG_MULTI_SZ)".into(),
        RegistryValueType::Dword => "a number (REG_DWORD)".into(),
        RegistryValueType::Binary => "bytes (REG_BINARY)".into(),
        RegistryValueType::Other(other) => format!("registry type {other}"),
    }
}

/// `text` as `REG_SZ` holds it: UTF-8 and a terminating NUL.
fn sz(text: &str) -> Vec<u8> {
    let mut data = text.as_bytes().to_vec();
    data.push(0);
    data
}

/// `items` as `REG_MULTI_SZ` holds them: each terminated, and the list too.
fn multi_sz(items: &[String]) -> Vec<u8> {
    let mut data = Vec::new();
    for item in items {
        data.extend_from_slice(item.as_bytes());
        data.push(0);
    }
    data.push(0);
    data
}

fn strings(data: &[u8]) -> Vec<String> {
    let data = data.strip_suffix(&[0]).unwrap_or(data);
    let data = data.strip_suffix(&[0]).unwrap_or(data);
    if data.is_empty() {
        return Vec::new();
    }
    data.split(|byte| *byte == 0).map(|item| String::from_utf8_lossy(item).into_owned()).collect()
}

fn dword(data: &[u8]) -> Option<u32> {
    Some(u32::from_le_bytes(data.try_into().ok()?))
}

fn hex(data: &[u8]) -> String {
    data.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::{FieldGroup, TakesEffect};

    fn sshd() -> Definition {
        let mut definition = Definition::new("sshd", Vec::new());
        definition.set("ImagePath", "/usr/libexec/openssh/start-sshd").unwrap();
        definition.set("Requires", "lpsd\n").unwrap();
        definition.set("Readiness", "alive").unwrap();
        definition
    }

    #[test]
    fn every_field_decodes_when_it_holds_what_its_kind_says() {
        // A plausible value of each field's kind, which the decoder must
        // take: the schema and the decoder say the same of every field.
        let plausible = |info: &FieldInfo| -> Vec<u8> {
            match (info.name, info.kind) {
                (_, FieldKind::Binary) => vec![1, 0, 4, 128],
                ("TTYPath", _) => sz("/dev/tty1"),
                ("Triggers", _) => multi_sz(&["boot".into(), "timer:daily".into()]),
                ("Environment", _) => multi_sz(&["A=1".into()]),
                ("SuccessExitCodes", _) => multi_sz(&["3".into()]),
                ("RuntimeDirectories", _) => multi_sz(&["thing".into()]),
                ("Conditions" | "Asserts", _) => multi_sz(&["path:/etc".into()]),
                ("ExecStartPre" | "ExecStartPost", _) => multi_sz(&["/bin/true".into()]),
                ("ExecReload" | "HealthCheck", _) => sz("/bin/true"),
                ("WorkingDirectory" | "ImagePath", _) => sz("/var"),
                (_, FieldKind::Text) => sz("lpsd"),
                (_, FieldKind::List) => multi_sz(&["lpsd".into()]),
                (_, FieldKind::Choice(names)) => names[names.len() - 1].0.to_le_bytes().to_vec(),
                (_, FieldKind::YesNo) => 1u32.to_le_bytes().to_vec(),
                (_, FieldKind::Number { .. }) => 7u32.to_le_bytes().to_vec(),
            }
        };
        for info in FIELDS {
            let mut definition = sshd();
            if info.name == "TTYPrecedence" {
                definition.set("TTYPath", "/dev/tty1").unwrap();
            }
            definition.unset(info.name);
            definition.values.push(RawRegistryValue { name: info.name.into(), value_type: info.kind.value_type(), data: plausible(info) });
            assert_eq!(definition.check(), Ok(()), "{} as {:?}", info.name, info.kind);
            // And of the wrong type, it says so beside the field.
            let wrong = if info.kind.value_type() == RegistryValueType::Dword { RegistryValueType::Sz } else { RegistryValueType::Dword };
            definition.unset(info.name);
            definition.values.push(RawRegistryValue { name: info.name.into(), value_type: wrong, data: 1u32.to_le_bytes().to_vec() });
            assert_eq!(definition.check().unwrap_err().field, Some(info.name), "{}", info.name);
        }
        assert_eq!(FIELDS.len(), 50);
        assert!(FIELDS.iter().filter(|info| info.group == FieldGroup::About).count() == 2);
        assert_eq!(service_field("imagepath").unwrap().takes_effect, TakesEffect::Restart);
    }

    #[test]
    fn values_are_stated_as_a_person_states_them_and_back() {
        let mut definition = sshd();
        assert_eq!(definition.text("ImagePath").as_deref(), Some("/usr/libexec/openssh/start-sshd"));
        assert_eq!(definition.get("ImagePath").unwrap().data.last(), Some(&0), "REG_SZ ends with its NUL");
        assert_eq!(definition.text("Readiness").as_deref(), Some("Alive"));
        definition.set("Arguments", "-D\n\n  -e  \n").unwrap();
        assert_eq!(definition.get("Arguments").unwrap().data, b"-D\0-e\0\0");
        assert_eq!(definition.text("Arguments").as_deref(), Some("-D\n-e"));
        definition.set("Disabled", "Yes").unwrap();
        assert_eq!(definition.text("Disabled").as_deref(), Some("yes"));
        assert_eq!(definition.set("StopTimeout", "ten").unwrap_err(), "StopTimeout is a whole number, from 0 to 4294967295.");
        assert_eq!(definition.set("Type", "forking").unwrap_err(), "Type is one of Simple, Oneshot.");
        definition.set("StopTimeout", " 20 ").unwrap();
        assert_eq!(definition.text("StopTimeout").as_deref(), Some("20"));
        // Nothing unsets it.
        definition.set("StopTimeout", "  ").unwrap();
        assert_eq!(definition.get("StopTimeout"), None);
        assert!(definition.set("ServiceSecurity", "O:SY").is_err());
        // A name kept in the case it was found in.
        definition.values.push(RawRegistryValue { name: "displayname".into(), value_type: RegistryValueType::Sz, data: sz("Old") });
        definition.set("DisplayName", "New").unwrap();
        assert_eq!(definition.values.iter().filter(|value| value.name.eq_ignore_ascii_case("DisplayName")).count(), 1);
        assert_eq!(definition.get("DisplayName").unwrap().name, "displayname");
    }

    #[test]
    fn what_is_wrong_is_said_beside_the_field_it_is_about() {
        let mut definition = Definition::new("sshd", Vec::new());
        assert_eq!(definition.check().unwrap_err(), Problem { field: Some("ImagePath"), message: "It needs a program to run: ImagePath is not set.".into() });
        definition.set("ImagePath", "start-sshd").unwrap();
        assert_eq!(definition.check().unwrap_err().field, Some("ImagePath"));
        definition.set("ImagePath", "/usr/sbin/sshd").unwrap();
        definition.set("Triggers", "boot\ntimer:every blue moon").unwrap();
        let problem = definition.check().unwrap_err();
        assert_eq!(problem.field, Some("Triggers"));
        assert!(problem.message.starts_with("“timer:every blue moon” is not a schedule: "), "{}", problem.message);
        definition.set("Triggers", "boot").unwrap();
        definition.set("Requires", "lpsd\nnot a name").unwrap();
        assert_eq!(definition.check().unwrap_err().field, Some("Requires"));
        let bad = Definition::new("has/slash", sshd().values);
        assert_eq!(bad.check().unwrap_err().field, Some("name"));
    }

    #[test]
    fn only_what_changed_is_written_and_the_rest_is_kept() {
        let mut found = sshd();
        found.values.push(RawRegistryValue { name: "LastTimerRun".into(), value_type: RegistryValueType::Other(11), data: vec![1; 8] });
        let mut edited = found.clone();
        edited.set("Readiness", "Alive").unwrap();
        assert_eq!(changes(&found, &edited), Vec::new(), "the same value is no change");
        edited.set("Readiness", "Notify").unwrap();
        edited.set("Requires", "").unwrap();
        edited.set("Description", "Remote logins").unwrap();
        let changed = changes(&found, &edited);
        assert_eq!(changed.len(), 3);
        assert!(changed.contains(&Change::Unset("Requires".into())));
        assert!(changed.iter().any(|change| matches!(change, Change::Set(value) if value.name == "Description")));
        assert_eq!(edited.others().map(|value| value.name.as_str()).collect::<Vec<_>>(), ["LastTimerRun"]);
    }
}
