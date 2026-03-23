//! Host-declared extension slots with cardinality enforcement.

use anyhow::{Result, bail};
use wasmtime::Engine;
use wasmtime::component::Component;

/// How many extensions may fill a slot simultaneously.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cardinality {
    ExactlyOne,
    AtLeastOne,
}

/// A host-declared slot that extensions can fill.
#[derive(Debug)]
pub struct SlotDefinition {
    pub name: &'static str,
    pub cardinality: Cardinality,
    pub required: bool,
}

/// All slots recognized by the host.
pub static SLOTS: &[SlotDefinition] = &[
    SlotDefinition {
        name: "session-provider",
        cardinality: Cardinality::ExactlyOne,
        required: true,
    },
    SlotDefinition {
        name: "compaction-provider",
        cardinality: Cardinality::ExactlyOne,
        required: true,
    },
    SlotDefinition {
        name: "llm-provider",
        cardinality: Cardinality::AtLeastOne,
        required: true,
    },
];

/// Looks up a slot definition by name.
pub fn find_slot(name: &str) -> Option<&'static SlotDefinition> {
    SLOTS.iter().find(|s| s.name == name)
}

/// Mapping from WIT export interface names to slot names.
///
/// The exact export name includes the package and version from `world.wit`.
/// We check for the slot-specific interface export to determine which slot
/// a component fills. General extensions export only `extension`.
const SLOT_EXPORTS: &[(&str, &str)] = &[
    ("ur:extension/llm-provider@0.3.0", "llm-provider"),
    ("ur:extension/session-provider@0.3.0", "session-provider"),
    (
        "ur:extension/compaction-provider@0.3.0",
        "compaction-provider",
    ),
];

/// Detects an extension's slot by inspecting its compiled component exports.
///
/// Returns `Some("llm-provider")` etc. for slot extensions, or `None` for
/// general extensions that only export the base `extension` interface.
pub fn detect_slot(engine: &Engine, component: &Component) -> Option<&'static str> {
    let ct = component.component_type();
    for &(export_name, slot_name) in SLOT_EXPORTS {
        if ct.get_export(engine, export_name).is_some() {
            return Some(slot_name);
        }
    }
    None
}

/// Validates that all required slots are satisfied.
///
/// Checks that each required slot has the minimum number of enabled
/// providers: exactly 1 for `ExactlyOne`, at least 1 for `AtLeastOne`.
///
/// # Errors
///
/// Returns an error describing which slots are unsatisfied.
pub fn validate_required_slots<'a>(
    entries: impl Iterator<Item = (&'a Option<String>, bool)>,
) -> Result<()> {
    // Count enabled providers per slot.
    let mut counts = std::collections::HashMap::<&str, usize>::new();
    for (slot, enabled) in entries {
        if let Some(ref name) = *slot
            && enabled
        {
            *counts.entry(name).or_default() += 1;
        }
    }

    let mut missing = Vec::new();
    for slot in SLOTS {
        if !slot.required {
            continue;
        }
        let count = counts.get(slot.name).copied().unwrap_or(0);
        let satisfied = match slot.cardinality {
            Cardinality::ExactlyOne => count == 1,
            Cardinality::AtLeastOne => count >= 1,
        };
        if !satisfied {
            missing.push(slot.name);
        }
    }

    if missing.is_empty() {
        Ok(())
    } else {
        bail!("required slots not satisfied: {}", missing.join(", "));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_slot_returns_known_slots() {
        for name in ["session-provider", "compaction-provider", "llm-provider"] {
            assert!(find_slot(name).is_some(), "expected Some for {name}");
            assert_eq!(find_slot(name).unwrap().name, name);
        }
    }

    #[test]
    fn find_slot_returns_none_for_unknown() {
        assert!(find_slot("bogus-slot").is_none());
    }

    // Helper: build entries for validate_required_slots
    fn entries(items: &[(&str, bool)]) -> Vec<(Option<String>, bool)> {
        items
            .iter()
            .map(|(slot, enabled)| (Some(slot.to_string()), *enabled))
            .collect()
    }

    #[test]
    fn validate_required_slots_passes_when_satisfied() {
        let e = entries(&[
            ("session-provider", true),
            ("compaction-provider", true),
            ("llm-provider", true),
        ]);
        validate_required_slots(e.iter().map(|(s, en)| (s, *en))).unwrap();
    }

    #[test]
    fn validate_required_slots_fails_exactly_one_zero_providers() {
        let e = entries(&[
            // no session-provider
            ("compaction-provider", true),
            ("llm-provider", true),
        ]);
        assert!(validate_required_slots(e.iter().map(|(s, en)| (s, *en))).is_err());
    }

    #[test]
    fn validate_required_slots_fails_exactly_one_two_providers() {
        let e = entries(&[
            ("session-provider", true),
            ("session-provider", true), // duplicate
            ("compaction-provider", true),
            ("llm-provider", true),
        ]);
        assert!(validate_required_slots(e.iter().map(|(s, en)| (s, *en))).is_err());
    }

    #[test]
    fn validate_required_slots_fails_at_least_one_zero_providers() {
        let e = entries(&[
            ("session-provider", true),
            ("compaction-provider", true),
            // no llm-provider
        ]);
        assert!(validate_required_slots(e.iter().map(|(s, en)| (s, *en))).is_err());
    }

    #[test]
    fn validate_required_slots_passes_at_least_one_multiple_providers() {
        let e = entries(&[
            ("session-provider", true),
            ("compaction-provider", true),
            ("llm-provider", true),
            ("llm-provider", true),
        ]);
        validate_required_slots(e.iter().map(|(s, en)| (s, *en))).unwrap();
    }

    #[test]
    fn validate_required_slots_disabled_not_counted() {
        let e = entries(&[
            ("session-provider", false), // disabled
            ("compaction-provider", true),
            ("llm-provider", true),
        ]);
        assert!(validate_required_slots(e.iter().map(|(s, en)| (s, *en))).is_err());
    }
}
