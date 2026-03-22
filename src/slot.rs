//! Host-declared extension slots with cardinality enforcement.

use anyhow::{Result, bail};

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

/// Validates that a slot name is known to the host.
///
/// # Errors
///
/// Returns an error if the slot name is not recognized.
pub fn validate_slot_name(name: &str) -> Result<()> {
    if find_slot(name).is_none() {
        bail!("unknown slot: {name}");
    }
    Ok(())
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

// Rust guideline compliant 2026-02-21
