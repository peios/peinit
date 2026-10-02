//! `svctl definition`: a service's definition, shown, checked, created,
//! changed and deleted, in its registry key.
//!
//! What a definition's values mean, how a person states them, and whether
//! peinit would take them are `client::Definition`'s, as Services Manager
//! uses them. A change is checked before anything is written, only the
//! values that change are written, and they are written in one transaction
//! that is refused if any of them changed since it was read.
//!
//! Built without `peios-registry` there is no registry to reach, and what
//! reaches it is used by the tests alone.
#![cfg_attr(not(feature = "peios-registry"), allow(dead_code))]

use std::io::Write;

use serde_json::{Map, Value, json};

use crate::client::{
    Change, Definition, FIELDS, FieldGroup, FieldKind, RawRegistryValue, TakesEffect, changes,
    service_field,
};

use super::command::{DefinitionAction, Edit, OutputMode};
use super::execute::{EXIT_OK, EXIT_SERVER_ERROR, EXIT_UNAVAILABLE};

/// Where definitions are kept.
pub(super) trait Store {
    /// The values of `service`'s definition, or `None` where it has none.
    fn read(&self, service: &str) -> Result<Option<Vec<RawRegistryValue>>, String>;
    /// Writes `changes`, refusing if a value changed is not as `found` has it.
    fn write(&self, service: &str, found: &[RawRegistryValue], changes: &[Change], create: bool) -> Result<(), String>;
    /// Deletes it. Whether another layer still defines it.
    fn delete(&self, service: &str) -> Result<bool, String>;
}

/// The registry, as this svctl was built to reach it.
#[cfg(feature = "peios-registry")]
struct Registry;

#[cfg(feature = "peios-registry")]
impl Store for Registry {
    fn read(&self, service: &str) -> Result<Option<Vec<RawRegistryValue>>, String> {
        crate::registry::read_service_values(service)
    }

    fn write(&self, service: &str, found: &[RawRegistryValue], changes: &[Change], create: bool) -> Result<(), String> {
        crate::registry::write_service_changes(service, found, changes, create)
    }

    fn delete(&self, service: &str) -> Result<bool, String> {
        crate::registry::delete_service_definition(service).map(|deleted| deleted == crate::registry::Deleted::StillDefined)
    }
}

pub(super) fn run(action: &DefinitionAction, service: &str, output: OutputMode, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    #[cfg(feature = "peios-registry")]
    {
        run_with(&Registry, action, service, output, out, err)
    }
    #[cfg(not(feature = "peios-registry"))]
    {
        let _ = (action, service, output, out);
        let _ = writeln!(err, "svctl: this svctl was built without the registry, and cannot reach a definition");
        EXIT_UNAVAILABLE
    }
}

pub(super) fn run_with(store: &dyn Store, action: &DefinitionAction, service: &str, output: OutputMode, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let fail = |err: &mut dyn Write, why: &str| {
        let _ = writeln!(err, "svctl: {service}: {why}");
        EXIT_SERVER_ERROR
    };
    let found = match store.read(service) {
        Ok(found) => found,
        Err(why) => {
            let _ = writeln!(err, "svctl: {service}: {why}");
            return EXIT_UNAVAILABLE;
        }
    };
    match (action, found) {
        (DefinitionAction::Create(_), Some(_)) => fail(err, "it is defined already; definition edit changes it"),
        (DefinitionAction::Create(edits), None) => change(store, service, &[], edits, true, output, out, err),
        (_, None) => fail(err, "it is not defined"),
        (DefinitionAction::Show, Some(values)) => {
            let definition = Definition::new(service, values);
            let _ = match output {
                OutputMode::Human => write!(out, "{}", show(&definition)),
                OutputMode::Json => writeln!(out, "{}", show_json(&definition)),
            };
            EXIT_OK
        }
        (DefinitionAction::Validate, Some(values)) => {
            let checked = Definition::new(service, values).check();
            let _ = match (output, &checked) {
                (OutputMode::Human, Ok(())) => writeln!(out, "{service}: peinit would take this definition."),
                (OutputMode::Human, Err(problem)) => writeln!(out, "{service}: {problem}"),
                (OutputMode::Json, _) => writeln!(out, "{}", json!({ "service": service, "valid": checked.is_ok(), "problem": problem_json(&checked) })),
            };
            if checked.is_ok() { EXIT_OK } else { EXIT_SERVER_ERROR }
        }
        (DefinitionAction::Edit(edits), Some(values)) => change(store, service, &values, edits, false, output, out, err),
        (DefinitionAction::Delete, Some(_)) => match store.delete(service) {
            Ok(false) => {
                let _ = match output {
                    OutputMode::Human => writeln!(out, "{service}: its definition is deleted. If it is running, it carries on until it stops."),
                    OutputMode::Json => writeln!(out, "{}", json!({ "service": service, "deleted": true, "still_defined": false })),
                };
                EXIT_OK
            }
            Ok(true) => {
                if output == OutputMode::Json {
                    let _ = writeln!(out, "{}", json!({ "service": service, "deleted": true, "still_defined": true }));
                }
                fail(err, "its definition is deleted where svctl writes it, but another layer of the registry defines it too, and that is there still")
            }
            Err(why) => fail(err, &why),
        },
    }
}

/// Makes `edits` to the definition found as `found`, checks it, and writes
/// what changed.
#[allow(clippy::too_many_arguments)]
fn change(store: &dyn Store, service: &str, found: &[RawRegistryValue], edits: &[Edit], create: bool, output: OutputMode, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let before = Definition::new(service, found.to_vec());
    let mut after = before.clone();
    if let Err(why) = apply(&mut after, edits) {
        let _ = writeln!(err, "svctl: {why}");
        return EXIT_SERVER_ERROR;
    }
    if let Err(problem) = after.check() {
        let _ = writeln!(err, "svctl: {service}: {problem} Nothing was written.");
        return EXIT_SERVER_ERROR;
    }
    let changed = changes(&before, &after);
    if changed.is_empty() && !create {
        let _ = writeln!(out, "{service}: nothing to change.");
        return EXIT_OK;
    }
    if let Err(why) = store.write(service, found, &changed, create) {
        let _ = writeln!(err, "svctl: {service}: {why}");
        return EXIT_SERVER_ERROR;
    }
    let name = |change: &Change| match change {
        Change::Set(value) => value.name.clone(),
        Change::Unset(name) => name.clone(),
    };
    let set: Vec<String> = changed.iter().filter(|change| matches!(change, Change::Set(_))).map(name).collect();
    let unset: Vec<String> = changed.iter().filter(|change| matches!(change, Change::Unset(_))).map(name).collect();
    // What a running service holds to until it is restarted.
    let held: Vec<String> = changed
        .iter()
        .map(name)
        .filter(|name| service_field(name).is_some_and(|info| info.takes_effect != TakesEffect::Runtime))
        .collect();
    let _ = match output {
        OutputMode::Json => writeln!(out, "{}", json!({ "service": service, "created": create, "set": set, "unset": unset, "at_restart": held })),
        OutputMode::Human => {
            let mut said = if create { format!("{service}: defined, with {}.", set.join(", ")) } else { format!("{service}:") };
            if !create {
                if !set.is_empty() {
                    said += &format!(" set {}", set.join(", "));
                }
                if !unset.is_empty() {
                    said += &format!("{} unset {}", if set.is_empty() { "" } else { ";" }, unset.join(", "));
                }
                said += ".";
            }
            if !create && !held.is_empty() {
                said += &format!("\nIf it is running, it keeps its {} as they were until it is restarted.", held.join(", "));
            }
            writeln!(out, "{said}")
        }
    };
    EXIT_OK
}

/// Makes `edits`, in order. Each `--set` of a list field is one item of it.
fn apply(definition: &mut Definition, edits: &[Edit]) -> Result<(), String> {
    let mut lists: Vec<(&'static str, Vec<String>)> = Vec::new();
    for edit in edits {
        match edit {
            Edit::Set { field, value } => {
                let Some(info) = service_field(field) else { return Err(format!("{field} is not a field of a service definition")) };
                match info.kind {
                    FieldKind::Binary => return Err(format!("{} is not set with svctl: reg set sets it, or Services Manager's permissions", info.name)),
                    FieldKind::List => match lists.iter_mut().find(|(name, _)| *name == info.name) {
                        Some((_, items)) => items.push(value.clone()),
                        None => lists.push((info.name, vec![value.clone()])),
                    },
                    _ => definition.set(info.name, value)?,
                }
            }
            Edit::Unset { field } => {
                definition.unset(field);
                lists.retain(|(name, _)| !name.eq_ignore_ascii_case(field));
            }
        }
    }
    for (field, items) in lists {
        let items: Vec<String> = items.into_iter().filter(|item| !item.is_empty()).collect();
        if items.is_empty() {
            definition.unset(field);
        } else {
            definition.set_list(field, &items)?;
        }
    }
    Ok(())
}

/// The definition as text: its fields that are set, by group, and the
/// values that are not fields, then whether peinit would take it.
fn show(definition: &Definition) -> String {
    let mut text = definition.name.clone();
    if let Some(title) = definition.text("DisplayName") {
        text += &format!(" ({title})");
    }
    text.push('\n');
    let line = |name: &str, value: &str| {
        let mut lines = value.lines();
        let mut said = format!("  {name:<22}{}\n", lines.next().unwrap_or(""));
        for more in lines {
            said += &format!("  {:<22}{more}\n", "");
        }
        said
    };
    for group in FieldGroup::ALL {
        let set: Vec<_> = FIELDS.iter().filter(|info| info.group == group).filter_map(|info| definition.text(info.name).map(|value| (info, value))).collect();
        if set.is_empty() {
            continue;
        }
        text += &format!("\n{}\n", group.title());
        for (info, value) in set {
            let value = match info.kind {
                FieldKind::Binary => format!("{} bytes", definition.get(info.name).map_or(0, |value| value.data.len())),
                FieldKind::Number { unit: Some(unit) } => format!("{value} {unit}"),
                _ => value,
            };
            text += &line(info.name, &value);
        }
    }
    let others: Vec<_> = definition.others().collect();
    if !others.is_empty() {
        text += "\nNot fields of a definition, kept as they are\n";
        for value in others {
            text += &line(&value.name, &format!("{} bytes", value.data.len()));
        }
    }
    match definition.check() {
        Ok(()) => text += "\npeinit would take this definition.\n",
        Err(problem) => text += &format!("\npeinit would not take this definition: {problem}\n"),
    }
    text
}

fn show_json(definition: &Definition) -> Value {
    let mut values = Map::new();
    for info in FIELDS {
        let Some(text) = definition.text(info.name) else { continue };
        let value = match info.kind {
            FieldKind::List => json!(text.lines().collect::<Vec<_>>()),
            FieldKind::Number { .. } => text.parse::<u64>().map_or(json!(text), |number| json!(number)),
            FieldKind::YesNo if text == "yes" || text == "no" => json!(text == "yes"),
            _ => json!(text),
        };
        values.insert(info.name.into(), value);
    }
    let others: Vec<Value> = definition.others().map(|value| json!({ "name": value.name, "bytes": value.data.len() })).collect();
    let checked = definition.check();
    json!({ "service": definition.name, "values": values, "others": others, "valid": checked.is_ok(), "problem": problem_json(&checked) })
}

fn problem_json(checked: &Result<(), crate::client::Problem>) -> Value {
    match checked {
        Ok(()) => Value::Null,
        Err(problem) => json!({ "field": problem.field, "message": problem.message }),
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::collections::BTreeMap;

    use super::*;

    /// Definitions kept in memory, as the registry keeps them.
    #[derive(Default)]
    struct Kept(RefCell<BTreeMap<String, Vec<RawRegistryValue>>>);

    impl Store for Kept {
        fn read(&self, service: &str) -> Result<Option<Vec<RawRegistryValue>>, String> {
            Ok(self.0.borrow().get(service).cloned())
        }

        fn write(&self, service: &str, found: &[RawRegistryValue], changes: &[Change], create: bool) -> Result<(), String> {
            let mut kept = self.0.borrow_mut();
            let now = kept.entry(service.into()).or_default();
            assert!(create || now == found);
            let mut definition = Definition::new(service, now.clone());
            for change in changes {
                match change {
                    Change::Set(value) => {
                        definition.unset(&value.name);
                        definition.values.push(value.clone());
                    }
                    Change::Unset(name) => definition.unset(name),
                }
            }
            *now = definition.values;
            Ok(())
        }

        fn delete(&self, service: &str) -> Result<bool, String> {
            self.0.borrow_mut().remove(service);
            Ok(false)
        }
    }

    fn run(store: &Kept, action: DefinitionAction, output: OutputMode) -> (i32, String, String) {
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = run_with(store, &action, "web", output, &mut out, &mut err);
        (code, String::from_utf8(out).unwrap(), String::from_utf8(err).unwrap())
    }

    fn set(field: &str, value: &str) -> Edit {
        Edit::Set { field: field.into(), value: value.into() }
    }

    #[test]
    fn a_definition_is_made_shown_changed_and_deleted() {
        let store = Kept::default();
        // Made, with what peinit needs: without a program, nothing is written.
        let (code, _, err) = run(&store, DefinitionAction::Create(vec![set("DisplayName", "Web")]), OutputMode::Human);
        assert_eq!(code, EXIT_SERVER_ERROR);
        assert_eq!(err, "svctl: web: It needs a program to run: ImagePath is not set. Nothing was written.\n");
        assert!(store.0.borrow().is_empty());
        let made = DefinitionAction::Create(vec![set("ImagePath", "/usr/bin/web"), set("Requires", "lpsd"), set("Requires", "netd:routed")]);
        let (code, out, _) = run(&store, made, OutputMode::Human);
        assert_eq!((code, out.as_str()), (EXIT_OK, "web: defined, with ImagePath, Requires.\n"));
        // Shown by group, a list a line an item.
        let (_, out, _) = run(&store, DefinitionAction::Show, OutputMode::Human);
        assert_eq!(
            out,
            "web\n\nExecution\n  ImagePath             /usr/bin/web\n\nDependencies\n  Requires              lpsd\n                        netd:routed\n\npeinit would take this definition.\n"
        );
        let (_, out, _) = run(&store, DefinitionAction::Show, OutputMode::Json);
        let shown: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(shown["values"]["Requires"], json!(["lpsd", "netd:routed"]));
        assert_eq!(shown["valid"], json!(true));
        // Changed: what a running service holds to is said.
        let (code, out, _) = run(&store, DefinitionAction::Edit(vec![set("StopTimeout", "20"), set("Identity", "SYSTEM"), Edit::Unset { field: "Requires".into() }]), OutputMode::Human);
        assert_eq!(code, EXIT_OK);
        assert_eq!(out, "web: set StopTimeout, Identity; unset Requires.\nIf it is running, it keeps its Identity, Requires as they were until it is restarted.\n");
        let (code, out, _) = run(&store, DefinitionAction::Edit(vec![set("StopTimeout", "20")]), OutputMode::Human);
        assert_eq!((code, out.as_str()), (EXIT_OK, "web: nothing to change.\n"));
        // A bad value says what is wrong, and changes nothing.
        let (code, _, err) = run(&store, DefinitionAction::Edit(vec![set("Type", "forking")]), OutputMode::Human);
        assert_eq!((code, err.as_str()), (EXIT_SERVER_ERROR, "svctl: Type is one of Simple, Oneshot.\n"));
        let (code, _, err) = run(&store, DefinitionAction::Edit(vec![set("ServiceSecurity", "O:SY")]), OutputMode::Human);
        assert_eq!(code, EXIT_SERVER_ERROR);
        assert!(err.contains("not set with svctl"));
        assert_eq!(run(&store, DefinitionAction::Create(vec![set("ImagePath", "/x")]), OutputMode::Human).2, "svctl: web: it is defined already; definition edit changes it\n");
        // Deleted, and then there is nothing to show.
        assert_eq!(run(&store, DefinitionAction::Delete, OutputMode::Human).0, EXIT_OK);
        assert_eq!(run(&store, DefinitionAction::Show, OutputMode::Human).2, "svctl: web: it is not defined\n");
    }

    #[test]
    fn validate_says_what_peinit_would_say() {
        let store = Kept::default();
        let mut bad = Definition::new("web", Vec::new());
        bad.set("ImagePath", "/usr/bin/web").unwrap();
        bad.set("Triggers", "timer:whenever").unwrap();
        store.0.borrow_mut().insert("web".into(), bad.values);
        let (code, out, _) = run(&store, DefinitionAction::Validate, OutputMode::Json);
        assert_eq!(code, EXIT_SERVER_ERROR);
        let said: Value = serde_json::from_str(&out).unwrap();
        assert_eq!((said["valid"].clone(), said["problem"]["field"].clone()), (json!(false), json!("Triggers")));
        let (_, out, _) = run(&store, DefinitionAction::Validate, OutputMode::Human);
        assert!(out.starts_with("web: “timer:whenever” is not a schedule: "), "{out}");
    }
}
