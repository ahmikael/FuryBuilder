//! Deterministic diff between two exact-version catalogs.
//!
//! The client JAR of one version cannot say how an id was named in another version, so a diff
//! is the only place where renames can be inferred. Known vanilla renames are listed
//! explicitly; other pairs are only called renamed when icon, display name, tags and block/item
//! form all match and the pairing is unique in both directions. Anything else stays in
//! `added`/`removed`, so an unknown id is never silently substituted.
//!
//! Diffing is a read-only operation. It does not modify either snapshot and does not write
//! aliases into `CatalogEntry`; imported states remain governed by the target write registry.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::catalog::{CatalogEntry, CatalogSnapshot, CatalogStates};

/// Renames verified for the modern catalog range. Both ids must be present as removed/added
/// for the pair to be reported; a common entry is never treated as renamed.
const KNOWN_VANILLA_RENAMES: &[(&str, &str)] = &[
    ("minecraft:grass", "minecraft:short_grass"),
    ("minecraft:scute", "minecraft:turtle_scute"),
];

/// A deterministic difference between two exact-version catalogs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogDiff {
    pub from_catalog_id: String,
    pub to_catalog_id: String,
    pub from_version: String,
    pub to_version: String,
    pub from_data_version: i32,
    pub to_data_version: i32,
    pub from_schema_version: u32,
    pub to_schema_version: u32,
    /// Entries only in the target snapshot.
    pub added: Vec<CatalogDiffEntry>,
    /// Entries only in the source snapshot. These ids are unknown for the target version and
    /// must not be mapped to another block without an explicit rename or user action.
    pub removed: Vec<CatalogDiffEntry>,
    /// Rename hints. `evidence.known_mapping` marks the verified vanilla pairs; other entries
    /// are conservative exact matches and still remain unproven aliases.
    pub renamed: Vec<CatalogRename>,
    /// Common entries whose contents changed. Sorted by id.
    pub changed: Vec<CatalogEntryChange>,
    /// Common entries with no reported change.
    pub unchanged: usize,
}

/// A compact identity for one added or removed entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogDiffEntry {
    pub id: String,
    pub display_name: Option<String>,
    pub block: bool,
    pub item: bool,
    /// False for entries from a non-vanilla namespace.
    pub vanilla: bool,
}

impl From<&CatalogEntry> for CatalogDiffEntry {
    fn from(entry: &CatalogEntry) -> Self {
        Self {
            id: entry.id.clone(),
            display_name: entry.display_name.clone(),
            block: entry.is_block(),
            item: entry.is_item(),
            vanilla: entry.id.starts_with("minecraft:"),
        }
    }
}

/// One rename hint between a removed id and an added id.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogRename {
    pub from: CatalogDiffEntry,
    pub to: CatalogDiffEntry,
    pub evidence: CatalogRenameEvidence,
}

/// Facts supporting one rename hint. `known_mapping` is the only verified evidence; the
/// other fields describe why a conservative match was possible.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogRenameEvidence {
    pub known_mapping: bool,
    pub same_icon: bool,
    pub same_display_name: bool,
    pub same_tags: bool,
}

/// Changed contents of one entry present in both catalogs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogEntryChange {
    pub id: String,
    pub display_name: Option<CatalogStringChange>,
    pub category: Option<CatalogStringChange>,
    pub icon_changed: bool,
    pub aliases_added: Vec<String>,
    pub aliases_removed: Vec<String>,
    pub tags_added: Vec<String>,
    pub tags_removed: Vec<String>,
    pub states: Option<CatalogStatesChange>,
    pub item_form_changed: bool,
    pub flags_changed: bool,
    pub legacy_id_changed: bool,
}

/// An optional string value that differs between the two snapshots.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogStringChange {
    pub from: Option<String>,
    pub to: Option<String>,
}

/// State differences for one entry. `block_form_changed` is true when the entry gained or
/// lost its block form; all other fields are then empty/false.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogStatesChange {
    pub block_form_changed: bool,
    pub properties_added: Vec<String>,
    pub properties_removed: Vec<String>,
    pub property_values_changed: Vec<CatalogPropertyValuesChange>,
    pub default_properties_changed: bool,
    pub property_values_complete_changed: bool,
    pub variant_count_changed: bool,
    pub multipart_changed: bool,
    pub variants_changed: bool,
    pub model_changed: bool,
}

/// One property whose accepted values differ between the two snapshots.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogPropertyValuesChange {
    pub key: String,
    pub from: Vec<String>,
    pub to: Vec<String>,
}

impl CatalogStatesChange {
    fn is_empty(&self) -> bool {
        !self.block_form_changed
            && self.properties_added.is_empty()
            && self.properties_removed.is_empty()
            && self.property_values_changed.is_empty()
            && !self.default_properties_changed
            && !self.property_values_complete_changed
            && !self.variant_count_changed
            && !self.multipart_changed
            && !self.variants_changed
            && !self.model_changed
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum CatalogDiffError {
    #[error("catalog {catalog} contains duplicate entry id `{id}`")]
    DuplicateEntry { catalog: &'static str, id: String },
}

impl CatalogDiff {
    /// Compares two snapshots without mutating either of them.
    pub fn between(from: &CatalogSnapshot, to: &CatalogSnapshot) -> Result<Self, CatalogDiffError> {
        let from_entries = index_entries(&from.entries, "from")?;
        let to_entries = index_entries(&to.entries, "to")?;

        let mut removed_ids: BTreeSet<String> = from_entries
            .keys()
            .filter(|id| !to_entries.contains_key(*id))
            .cloned()
            .collect();
        let mut added_ids: BTreeSet<String> = to_entries
            .keys()
            .filter(|id| !from_entries.contains_key(*id))
            .cloned()
            .collect();

        let mut renamed = Vec::new();
        let mut paired_from = BTreeSet::new();
        let mut paired_to = BTreeSet::new();

        for (old_id, new_id) in KNOWN_VANILLA_RENAMES {
            push_known_rename(
                &from_entries,
                &to_entries,
                &removed_ids,
                &added_ids,
                old_id,
                new_id,
                &mut renamed,
                &mut paired_from,
                &mut paired_to,
            );
            push_known_rename(
                &from_entries,
                &to_entries,
                &removed_ids,
                &added_ids,
                new_id,
                old_id,
                &mut renamed,
                &mut paired_from,
                &mut paired_to,
            );
        }

        for (old_id, new_id) in inferred_renames(
            &from_entries,
            &to_entries,
            &removed_ids,
            &added_ids,
            &paired_from,
            &paired_to,
        ) {
            let (Some(old), Some(new)) = (from_entries.get(&old_id), to_entries.get(&new_id))
            else {
                continue;
            };
            renamed.push(CatalogRename {
                from: CatalogDiffEntry::from(*old),
                to: CatalogDiffEntry::from(*new),
                evidence: CatalogRenameEvidence {
                    known_mapping: false,
                    same_icon: true,
                    same_display_name: true,
                    same_tags: true,
                },
            });
            paired_from.insert(old_id.clone());
            paired_to.insert(new_id.clone());
        }

        removed_ids.retain(|id| !paired_from.contains(id));
        added_ids.retain(|id| !paired_to.contains(id));

        renamed.sort_by(|left, right| {
            left.from
                .id
                .cmp(&right.from.id)
                .then_with(|| left.to.id.cmp(&right.to.id))
        });

        let added = added_ids
            .iter()
            .filter_map(|id| to_entries.get(id))
            .map(|entry| CatalogDiffEntry::from(*entry))
            .collect();
        let removed = removed_ids
            .iter()
            .filter_map(|id| from_entries.get(id))
            .map(|entry| CatalogDiffEntry::from(*entry))
            .collect();

        let mut changed = Vec::new();
        let mut unchanged = 0;
        for (id, old) in &from_entries {
            let Some(new) = to_entries.get(id) else {
                continue;
            };
            match diff_entry(id, old, new) {
                Some(change) => changed.push(change),
                None => unchanged += 1,
            }
        }
        changed.sort_by(|left, right| left.id.cmp(&right.id));

        Ok(Self {
            from_catalog_id: from.catalog_id.clone(),
            to_catalog_id: to.catalog_id.clone(),
            from_version: from.minecraft_version.clone(),
            to_version: to.minecraft_version.clone(),
            from_data_version: from.data_version,
            to_data_version: to.data_version,
            from_schema_version: from.schema_version,
            to_schema_version: to.schema_version,
            added,
            removed,
            renamed,
            changed,
            unchanged,
        })
    }
}

fn index_entries<'a>(
    entries: &'a [CatalogEntry],
    catalog: &'static str,
) -> Result<BTreeMap<String, &'a CatalogEntry>, CatalogDiffError> {
    let mut index = BTreeMap::new();
    for entry in entries {
        if index.insert(entry.id.clone(), entry).is_some() {
            return Err(CatalogDiffError::DuplicateEntry {
                catalog,
                id: entry.id.clone(),
            });
        }
    }
    Ok(index)
}

#[allow(clippy::too_many_arguments)]
fn push_known_rename(
    from_entries: &BTreeMap<String, &CatalogEntry>,
    to_entries: &BTreeMap<String, &CatalogEntry>,
    removed_ids: &BTreeSet<String>,
    added_ids: &BTreeSet<String>,
    old_id: &str,
    new_id: &str,
    renamed: &mut Vec<CatalogRename>,
    paired_from: &mut BTreeSet<String>,
    paired_to: &mut BTreeSet<String>,
) {
    if !removed_ids.contains(old_id) || !added_ids.contains(new_id) {
        return;
    }
    if paired_from.contains(old_id) || paired_to.contains(new_id) {
        return;
    }
    let (Some(old), Some(new)) = (from_entries.get(old_id), to_entries.get(new_id)) else {
        return;
    };
    renamed.push(CatalogRename {
        from: CatalogDiffEntry::from(*old),
        to: CatalogDiffEntry::from(*new),
        evidence: CatalogRenameEvidence {
            known_mapping: true,
            same_icon: same_nonempty_option(&old.icon_texture, &new.icon_texture),
            same_display_name: same_nonempty_option(&old.display_name, &new.display_name),
            same_tags: same_tags(&old.tags, &new.tags) && !old.tags.is_empty(),
        },
    });
    paired_from.insert(old_id.to_owned());
    paired_to.insert(new_id.to_owned());
}

type RenameKey = (String, String, Vec<String>, bool, bool);

fn inferred_renames(
    from_entries: &BTreeMap<String, &CatalogEntry>,
    to_entries: &BTreeMap<String, &CatalogEntry>,
    removed_ids: &BTreeSet<String>,
    added_ids: &BTreeSet<String>,
    paired_from: &BTreeSet<String>,
    paired_to: &BTreeSet<String>,
) -> Vec<(String, String)> {
    let mut removed: BTreeMap<RenameKey, Vec<String>> = BTreeMap::new();
    let mut added: BTreeMap<RenameKey, Vec<String>> = BTreeMap::new();

    for id in removed_ids {
        if paired_from.contains(id) {
            continue;
        }
        let Some(entry) = from_entries.get(id.as_str()) else {
            continue;
        };
        if let Some(key) = rename_key(entry) {
            removed.entry(key).or_default().push(id.clone());
        }
    }
    for id in added_ids {
        if paired_to.contains(id) {
            continue;
        }
        let Some(entry) = to_entries.get(id.as_str()) else {
            continue;
        };
        if let Some(key) = rename_key(entry) {
            added.entry(key).or_default().push(id.clone());
        }
    }

    let mut pairs = Vec::new();
    for (key, removed_ids) in removed {
        let Some(added_ids) = added.get(&key) else {
            continue;
        };
        if removed_ids.len() != 1 || added_ids.len() != 1 {
            continue;
        }
        pairs.push((removed_ids[0].clone(), added_ids[0].clone()));
    }
    pairs
}

fn rename_key(entry: &CatalogEntry) -> Option<RenameKey> {
    if !entry.id.starts_with("minecraft:") {
        return None;
    }
    let icon = entry.icon_texture.as_ref()?;
    let display_name = entry.display_name.as_ref()?;
    if icon.is_empty() || display_name.is_empty() || entry.tags.is_empty() {
        return None;
    }
    Some((
        icon.clone(),
        display_name.clone(),
        normalized_values(&entry.tags),
        entry.is_block(),
        entry.is_item(),
    ))
}

fn diff_entry(id: &str, from: &CatalogEntry, to: &CatalogEntry) -> Option<CatalogEntryChange> {
    let display_name = string_change(&from.display_name, &to.display_name);
    let category = string_change(&from.category, &to.category);
    let icon_changed = from.icon_texture != to.icon_texture;
    let aliases_added = added_values(&from.aliases, &to.aliases);
    let aliases_removed = added_values(&to.aliases, &from.aliases);
    let tags_added = added_values(&from.tags, &to.tags);
    let tags_removed = added_values(&to.tags, &from.tags);
    let states = diff_states(from.states.as_ref(), to.states.as_ref());
    let item_form_changed = from.item_form != to.item_form;
    let flags_changed = from.flags != to.flags;
    let legacy_id_changed = from.legacy_id != to.legacy_id;

    if display_name.is_none()
        && category.is_none()
        && !icon_changed
        && aliases_added.is_empty()
        && aliases_removed.is_empty()
        && tags_added.is_empty()
        && tags_removed.is_empty()
        && states.is_none()
        && !item_form_changed
        && !flags_changed
        && !legacy_id_changed
    {
        return None;
    }

    Some(CatalogEntryChange {
        id: id.to_owned(),
        display_name,
        category,
        icon_changed,
        aliases_added,
        aliases_removed,
        tags_added,
        tags_removed,
        states,
        item_form_changed,
        flags_changed,
        legacy_id_changed,
    })
}

fn string_change(from: &Option<String>, to: &Option<String>) -> Option<CatalogStringChange> {
    if from == to {
        return None;
    }
    Some(CatalogStringChange {
        from: from.clone(),
        to: to.clone(),
    })
}

fn diff_states(
    from: Option<&CatalogStates>,
    to: Option<&CatalogStates>,
) -> Option<CatalogStatesChange> {
    if from == to {
        return None;
    }
    let Some(from) = from else {
        return Some(CatalogStatesChange {
            block_form_changed: true,
            properties_added: Vec::new(),
            properties_removed: Vec::new(),
            property_values_changed: Vec::new(),
            default_properties_changed: false,
            property_values_complete_changed: false,
            variant_count_changed: false,
            multipart_changed: false,
            variants_changed: false,
            model_changed: false,
        });
    };
    let Some(to) = to else {
        return Some(CatalogStatesChange {
            block_form_changed: true,
            properties_added: Vec::new(),
            properties_removed: Vec::new(),
            property_values_changed: Vec::new(),
            default_properties_changed: false,
            property_values_complete_changed: false,
            variant_count_changed: false,
            multipart_changed: false,
            variants_changed: false,
            model_changed: false,
        });
    };

    let mut properties_added = Vec::new();
    let mut properties_removed = Vec::new();
    let mut property_values_changed = Vec::new();
    let mut keys: BTreeSet<&String> = from.properties.keys().collect();
    keys.extend(to.properties.keys());
    for key in keys {
        match (from.properties.get(key), to.properties.get(key)) {
            (None, Some(_)) => {
                properties_added.push(key.clone());
            }
            (Some(_), None) => {
                properties_removed.push(key.clone());
            }
            (Some(old_values), Some(new_values)) => {
                let old = normalized_values(old_values);
                let new = normalized_values(new_values);
                if old != new {
                    property_values_changed.push(CatalogPropertyValuesChange {
                        key: key.clone(),
                        from: old,
                        to: new,
                    });
                }
            }
            (None, None) => {}
        }
    }

    let change = CatalogStatesChange {
        block_form_changed: false,
        properties_added,
        properties_removed,
        property_values_changed,
        default_properties_changed: from.default_properties != to.default_properties,
        property_values_complete_changed: from.property_values_complete
            != to.property_values_complete,
        variant_count_changed: from.variant_count != to.variant_count,
        multipart_changed: from.multipart != to.multipart,
        variants_changed: from.variants != to.variants,
        model_changed: from.model != to.model,
    };
    (!change.is_empty()).then_some(change)
}

fn same_nonempty_option(left: &Option<String>, right: &Option<String>) -> bool {
    matches!((left, right), (Some(left), Some(right)) if !left.is_empty() && left == right)
}

fn same_tags(left: &[String], right: &[String]) -> bool {
    normalized_values(left) == normalized_values(right)
}

fn normalized_values(values: &[String]) -> Vec<String> {
    values
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn added_values(from: &[String], to: &[String]) -> Vec<String> {
    let existing: BTreeSet<&String> = from.iter().collect();
    to.iter()
        .filter(|value| !existing.contains(*value))
        .cloned()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::{
        CatalogCounts, CatalogFlags, CatalogItemForm, CatalogVariant, CatalogVariantModel,
    };
    use std::collections::BTreeMap;

    fn entry(
        id: &str,
        display_name: Option<&str>,
        icon: Option<&str>,
        tags: &[&str],
    ) -> CatalogEntry {
        CatalogEntry {
            id: id.to_owned(),
            legacy_id: None,
            display_name: display_name.map(str::to_owned),
            category: None,
            tags: tags.iter().map(|tag| (*tag).to_owned()).collect(),
            icon_texture: icon.map(str::to_owned),
            aliases: Vec::new(),
            states: None,
            item_form: None,
            flags: CatalogFlags::default(),
        }
    }

    fn block(mut entry: CatalogEntry, model: &str) -> CatalogEntry {
        entry.states = Some(CatalogStates {
            properties: BTreeMap::new(),
            property_values_complete: true,
            default_properties: None,
            variant_count: 1,
            variants: vec![CatalogVariant {
                properties: BTreeMap::new(),
                models: vec![CatalogVariantModel {
                    model: Some(model.to_owned()),
                    x_rotation: 0,
                    y_rotation: 0,
                    uvlock: false,
                    weight: 1,
                }],
            }],
            multipart: false,
            model: Some(model.to_owned()),
        });
        entry
    }

    fn item(mut entry: CatalogEntry) -> CatalogEntry {
        entry.item_form = Some(CatalogItemForm {
            item_definition: true,
            model_type: Some("minecraft:model".to_owned()),
            model: Some("minecraft:item/example".to_owned()),
        });
        entry
    }

    fn snapshot(id: &str, version: &str, entries: Vec<CatalogEntry>) -> CatalogSnapshot {
        CatalogSnapshot {
            schema_version: crate::catalog::CATALOG_SCHEMA_VERSION,
            catalog_id: id.to_owned(),
            catalog_hash: "0".repeat(64),
            minecraft_version: version.to_owned(),
            data_version: 1,
            client_jar: "client.jar".to_owned(),
            client_jar_sha1: None,
            resource_pack_format: None,
            datapack_format: None,
            entries,
            atlases: Vec::new(),
            counts: CatalogCounts::default(),
            warnings: Vec::new(),
            cache_hit: false,
        }
    }

    #[test]
    fn separates_added_removed_and_unchanged_entries() {
        let from = snapshot(
            "java-1.20.6-a",
            "1.20.6",
            vec![
                entry("minecraft:stone", Some("Stone"), Some("block/stone"), &[]),
                entry("minecraft:dirt", Some("Dirt"), Some("block/dirt"), &[]),
            ],
        );
        let to = snapshot(
            "java-1.21.4-b",
            "1.21.4",
            vec![
                entry("minecraft:stone", Some("Stone"), Some("block/stone"), &[]),
                entry(
                    "minecraft:copper_bulb",
                    Some("Copper Bulb"),
                    Some("block/copper_bulb"),
                    &[],
                ),
            ],
        );

        let diff = CatalogDiff::between(&from, &to).expect("diff");

        assert_eq!(diff.added.len(), 1);
        assert_eq!(diff.added[0].id, "minecraft:copper_bulb");
        assert_eq!(diff.removed.len(), 1);
        assert_eq!(diff.removed[0].id, "minecraft:dirt");
        assert!(diff.renamed.is_empty());
        assert!(diff.changed.is_empty());
        assert_eq!(diff.unchanged, 1);
        assert_eq!(diff.from_version, "1.20.6");
        assert_eq!(diff.to_version, "1.21.4");
    }

    #[test]
    fn detects_known_vanilla_rename() {
        let from = snapshot(
            "java-1.20.2-a",
            "1.20.2",
            vec![
                block(
                    item(entry(
                        "minecraft:grass",
                        Some("Grass"),
                        Some("block/grass"),
                        &["minecraft:replaceable"],
                    )),
                    "block/grass",
                ),
                block(
                    entry("minecraft:stone", Some("Stone"), Some("block/stone"), &[]),
                    "block/stone",
                ),
            ],
        );
        let to = snapshot(
            "java-1.20.3-b",
            "1.20.3",
            vec![
                block(
                    item(entry(
                        "minecraft:short_grass",
                        Some("Short Grass"),
                        Some("block/short_grass"),
                        &["minecraft:replaceable"],
                    )),
                    "block/short_grass",
                ),
                block(
                    entry("minecraft:stone", Some("Stone"), Some("block/stone"), &[]),
                    "block/stone",
                ),
            ],
        );

        let diff = CatalogDiff::between(&from, &to).expect("diff");

        assert!(diff.added.is_empty());
        assert!(diff.removed.is_empty());
        assert_eq!(diff.renamed.len(), 1);
        assert_eq!(diff.renamed[0].from.id, "minecraft:grass");
        assert_eq!(diff.renamed[0].to.id, "minecraft:short_grass");
        assert!(diff.renamed[0].evidence.known_mapping);
        assert!(!diff.renamed[0].evidence.same_display_name);
        assert!(diff.changed.is_empty());
        assert_eq!(diff.unchanged, 1);
    }

    #[test]
    fn infers_only_unique_exact_renames() {
        let from = snapshot(
            "java-a",
            "1.21.4",
            vec![entry(
                "minecraft:old_id",
                Some("Same Name"),
                Some("block/same"),
                &["minecraft:same_tag"],
            )],
        );
        let to = snapshot(
            "java-b",
            "1.21.5",
            vec![entry(
                "minecraft:new_id",
                Some("Same Name"),
                Some("block/same"),
                &["minecraft:same_tag"],
            )],
        );
        let diff = CatalogDiff::between(&from, &to).expect("diff");
        assert_eq!(diff.renamed.len(), 1);
        assert!(!diff.renamed[0].evidence.known_mapping);
        assert!(diff.renamed[0].evidence.same_icon);
        assert!(diff.renamed[0].evidence.same_display_name);
        assert!(diff.renamed[0].evidence.same_tags);

        let ambiguous_from = snapshot(
            "java-a",
            "1.21.4",
            vec![
                entry(
                    "minecraft:old_a",
                    Some("Same"),
                    Some("block/same"),
                    &["minecraft:tag"],
                ),
                entry(
                    "minecraft:old_b",
                    Some("Same"),
                    Some("block/same"),
                    &["minecraft:tag"],
                ),
            ],
        );
        let ambiguous_to = snapshot(
            "java-b",
            "1.21.5",
            vec![
                entry(
                    "minecraft:new_a",
                    Some("Same"),
                    Some("block/same"),
                    &["minecraft:tag"],
                ),
                entry(
                    "minecraft:new_b",
                    Some("Same"),
                    Some("block/same"),
                    &["minecraft:tag"],
                ),
            ],
        );
        let ambiguous = CatalogDiff::between(&ambiguous_from, &ambiguous_to).expect("diff");
        assert!(ambiguous.renamed.is_empty());
        assert_eq!(ambiguous.added.len(), 2);
        assert_eq!(ambiguous.removed.len(), 2);
    }

    #[test]
    fn never_infers_non_vanilla_renames() {
        let from = snapshot(
            "java-a",
            "1.21.4",
            vec![entry(
                "example:old",
                Some("Same"),
                Some("example:block"),
                &["example:tag"],
            )],
        );
        let to = snapshot(
            "java-b",
            "1.21.5",
            vec![entry(
                "example:new",
                Some("Same"),
                Some("example:block"),
                &["example:tag"],
            )],
        );
        let diff = CatalogDiff::between(&from, &to).expect("diff");
        assert!(diff.renamed.is_empty());
        assert!(!diff.added[0].vanilla);
        assert_eq!(diff.added[0].id, "example:new");
        assert_eq!(diff.removed[0].id, "example:old");
    }

    #[test]
    fn reports_display_category_tags_aliases_and_flags_changes() {
        let mut from = entry(
            "minecraft:stone",
            Some("Stone"),
            Some("block/stone"),
            &["a", "b"],
        );
        from.aliases = vec!["minecraft:old_stone".to_owned()];
        from.category = Some("natural".to_owned());
        from.flags.technical = Some(false);
        let mut to = entry(
            "minecraft:stone",
            Some("Stone Block"),
            Some("block/stone"),
            &["b", "c"],
        );
        to.aliases = vec!["minecraft:legacy_stone".to_owned()];
        to.category = Some("building".to_owned());
        to.flags.technical = Some(true);

        let diff = CatalogDiff::between(
            &snapshot("java-a", "1.21.4", vec![from]),
            &snapshot("java-b", "1.21.5", vec![to]),
        )
        .expect("diff");

        assert_eq!(diff.changed.len(), 1);
        let changed = &diff.changed[0];
        assert_eq!(
            changed.display_name,
            Some(CatalogStringChange {
                from: Some("Stone".to_owned()),
                to: Some("Stone Block".to_owned())
            })
        );
        assert_eq!(
            changed
                .category
                .as_ref()
                .and_then(|value| value.to.as_deref()),
            Some("building")
        );
        assert_eq!(changed.tags_added, ["c"]);
        assert_eq!(changed.tags_removed, ["a"]);
        assert_eq!(changed.aliases_added, ["minecraft:legacy_stone"]);
        assert_eq!(changed.aliases_removed, ["minecraft:old_stone"]);
        assert!(changed.flags_changed);
        assert_eq!(diff.unchanged, 0);
    }

    #[test]
    fn reports_state_changes_in_detail() {
        let mut from = block(
            entry("minecraft:lamp", Some("Lamp"), Some("block/lamp"), &[]),
            "block/lamp_off",
        );
        let from_states = from.states.as_mut().expect("states");
        from_states.properties.insert(
            "facing".to_owned(),
            vec!["north".to_owned(), "south".to_owned()],
        );
        from_states
            .properties
            .insert("lit".to_owned(), vec!["true".to_owned()]);
        from_states.default_properties =
            Some(BTreeMap::from([("facing".to_owned(), "north".to_owned())]));
        from_states.property_values_complete = false;

        let mut to = block(
            entry("minecraft:lamp", Some("Lamp"), Some("block/lamp"), &[]),
            "block/lamp_on",
        );
        let to_states = to.states.as_mut().expect("states");
        to_states.properties.insert(
            "facing".to_owned(),
            vec!["north".to_owned(), "east".to_owned()],
        );
        to_states.default_properties =
            Some(BTreeMap::from([("facing".to_owned(), "east".to_owned())]));
        to_states.property_values_complete = true;
        to_states.variant_count = 2;
        to_states.multipart = true;
        to_states.variants.push(CatalogVariant {
            properties: BTreeMap::new(),
            models: Vec::new(),
        });

        let diff = CatalogDiff::between(
            &snapshot("java-a", "1.21.4", vec![from]),
            &snapshot("java-b", "1.21.5", vec![to]),
        )
        .expect("diff");

        let states = diff.changed[0].states.as_ref().expect("state change");
        assert_eq!(states.properties_added, Vec::<String>::new());
        assert_eq!(states.properties_removed, ["lit"]);
        assert_eq!(states.property_values_changed.len(), 1);
        assert_eq!(states.property_values_changed[0].key, "facing");
        assert_eq!(states.property_values_changed[0].from, ["north", "south"]);
        assert_eq!(states.property_values_changed[0].to, ["east", "north"]);
        assert!(states.default_properties_changed);
        assert!(states.property_values_complete_changed);
        assert!(states.variant_count_changed);
        assert!(states.multipart_changed);
        assert!(states.variants_changed);
        assert!(states.model_changed);
        assert!(!states.block_form_changed);
    }

    #[test]
    fn reports_block_form_and_item_form_changes() {
        let mut from = item(entry(
            "minecraft:item",
            Some("Item"),
            Some("item/item"),
            &[],
        ));
        from.states = Some(CatalogStates {
            properties: BTreeMap::new(),
            property_values_complete: true,
            default_properties: None,
            variant_count: 1,
            variants: Vec::new(),
            multipart: false,
            model: None,
        });
        let mut to = entry("minecraft:item", Some("Item"), Some("item/item"), &[]);
        to.item_form = Some(CatalogItemForm {
            item_definition: false,
            model_type: None,
            model: Some("minecraft:item/other".to_owned()),
        });

        let diff = CatalogDiff::between(
            &snapshot("java-a", "1.21.4", vec![from]),
            &snapshot("java-b", "1.21.5", vec![to]),
        )
        .expect("diff");

        let changed = &diff.changed[0];
        let states = changed.states.as_ref().expect("state change");
        assert!(states.block_form_changed);
        assert!(changed.item_form_changed);
        assert_eq!(diff.unchanged, 0);
    }

    #[test]
    fn sorts_outputs_by_id() {
        let from = snapshot(
            "java-a",
            "1.21.4",
            vec![
                entry("minecraft:z_removed", Some("Z"), Some("block/z"), &[]),
                entry("minecraft:a_removed", Some("A"), Some("block/a"), &[]),
                entry("minecraft:m_changed", Some("M"), Some("block/m"), &[]),
            ],
        );
        let to = snapshot(
            "java-b",
            "1.21.5",
            vec![
                entry("minecraft:z_added", Some("Z"), Some("block/z2"), &[]),
                entry("minecraft:a_added", Some("A"), Some("block/a2"), &[]),
                entry("minecraft:m_changed", Some("M2"), Some("block/m2"), &[]),
            ],
        );
        let diff = CatalogDiff::between(&from, &to).expect("diff");
        assert_eq!(
            diff.added
                .iter()
                .map(|entry| entry.id.as_str())
                .collect::<Vec<_>>(),
            ["minecraft:a_added", "minecraft:z_added"]
        );
        assert_eq!(
            diff.removed
                .iter()
                .map(|entry| entry.id.as_str())
                .collect::<Vec<_>>(),
            ["minecraft:a_removed", "minecraft:z_removed"]
        );
        assert_eq!(diff.changed[0].id, "minecraft:m_changed");
    }

    #[test]
    fn rejects_duplicate_entry_ids() {
        let duplicate = snapshot(
            "java-a",
            "1.21.4",
            vec![
                entry("minecraft:stone", Some("Stone"), Some("block/stone"), &[]),
                entry(
                    "minecraft:stone",
                    Some("Stone again"),
                    Some("block/stone"),
                    &[],
                ),
            ],
        );
        let other = snapshot("java-b", "1.21.5", vec![]);
        let error = CatalogDiff::between(&duplicate, &other).expect_err("duplicate");
        assert_eq!(
            error,
            CatalogDiffError::DuplicateEntry {
                catalog: "from",
                id: "minecraft:stone".to_owned()
            }
        );
    }

    #[test]
    fn serializes_with_camel_case_contract() {
        let from = snapshot(
            "java-a",
            "1.20.2",
            vec![block(
                item(entry(
                    "minecraft:grass",
                    Some("Grass"),
                    Some("block/grass"),
                    &["minecraft:replaceable"],
                )),
                "block/grass",
            )],
        );
        let to = snapshot(
            "java-b",
            "1.20.3",
            vec![block(
                item(entry(
                    "minecraft:short_grass",
                    Some("Short Grass"),
                    Some("block/short_grass"),
                    &["minecraft:replaceable"],
                )),
                "block/short_grass",
            )],
        );
        let diff = CatalogDiff::between(&from, &to).expect("diff");
        let value = serde_json::to_value(diff).expect("json");
        assert_eq!(value["fromCatalogId"], "java-a");
        assert_eq!(value["renamed"][0]["from"]["id"], "minecraft:grass");
        assert_eq!(value["renamed"][0]["to"]["id"], "minecraft:short_grass");
        assert_eq!(value["renamed"][0]["evidence"]["knownMapping"], true);
        assert_eq!(value["renamed"][0]["evidence"]["sameDisplayName"], false);
        assert_eq!(value["added"], serde_json::json!([]));
        assert_eq!(value["removed"], serde_json::json!([]));
    }

    #[test]
    fn identical_snapshots_have_no_changes() {
        let snapshot = snapshot(
            "java-a",
            "1.21.4",
            vec![block(
                entry("minecraft:stone", Some("Stone"), Some("block/stone"), &[]),
                "block/stone",
            )],
        );
        let diff = CatalogDiff::between(&snapshot, &snapshot).expect("diff");
        assert!(diff.added.is_empty());
        assert!(diff.removed.is_empty());
        assert!(diff.renamed.is_empty());
        assert!(diff.changed.is_empty());
        assert_eq!(diff.unchanged, 1);
    }
}
