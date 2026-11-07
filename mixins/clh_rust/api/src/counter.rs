use crate::config::{Config, GroupKind};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Snapshot {
    pub center_chunk: [i32; 2],
    pub min_chunk: [i32; 2],
    pub area_size: i32,
    pub loaded_chunks: usize,
    pub entities: BTreeMap<String, u32>,
    pub blocks: BTreeMap<String, u32>,
    pub groups: BTreeMap<String, u32>,
    pub highlighted_entities: BTreeSet<u32>,
}

impl Snapshot {
    pub fn empty(config: &Config) -> Self {
        Self {
            area_size: config.chunk_area_size,
            entities: zeroed(&config.entities),
            blocks: zeroed(&config.blocks),
            groups: config.groups.keys().map(|name| (name.clone(), 0)).collect(),
            ..Self::default()
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct EntitySample<'a> {
    pub id: u32,
    pub kind: &'a str,
    pub chunk: [i32; 2],
    pub mob: bool,
    pub item: bool,
    pub minecart: bool,
}

pub fn count_entities<'a>(
    config: &Config,
    min_chunk: [i32; 2],
    area_size: i32,
    samples: impl IntoIterator<Item = EntitySample<'a>>,
) -> (BTreeMap<String, u32>, BTreeMap<String, u32>, BTreeSet<u32>) {
    let mut counts = zeroed(&config.entities);
    let mut groups: BTreeMap<String, u32> = config
        .groups
        .iter()
        .filter(|(_, group)| group.kind == GroupKind::Entity)
        .map(|(name, _)| (name.clone(), 0))
        .collect();
    let mut inside = Vec::new();

    for sample in samples {
        if !contains(min_chunk, area_size, sample.chunk) {
            continue;
        }
        increment(&mut counts, sample.kind, true);
        increment(&mut counts, "group:entities", true);
        increment(&mut counts, "group:mobs", sample.mob);
        increment(&mut counts, "group:items", sample.item);
        increment(&mut counts, "group:minecarts", sample.minecart);
        for (name, group) in &config.groups {
            if group.kind == GroupKind::Entity && group.members.iter().any(|id| id == sample.kind) {
                *groups.entry(name.clone()).or_default() += 1;
            }
        }
        inside.push(sample);
    }

    let mut highlighted = BTreeSet::new();
    if config.highlight_overloaded_entities {
        for sample in inside {
            if highlighted.len() >= config.max_highlighted_entities {
                break;
            }
            let exact = overloaded(&counts, &config.entities, sample.kind);
            let builtin = overloaded_if(&counts, &config.entities, "group:entities", true)
                || overloaded_if(&counts, &config.entities, "group:mobs", sample.mob)
                || overloaded_if(&counts, &config.entities, "group:items", sample.item)
                || overloaded_if(
                    &counts,
                    &config.entities,
                    "group:minecarts",
                    sample.minecart,
                );
            let custom = config.groups.iter().any(|(name, group)| {
                group.kind == GroupKind::Entity
                    && group.members.iter().any(|id| id == sample.kind)
                    && groups.get(name).copied().unwrap_or(0) > group.limit
            });
            if exact || builtin || custom {
                highlighted.insert(sample.id);
            }
        }
    }
    (counts, groups, highlighted)
}

pub fn count_block(
    config: &Config,
    counts: &mut BTreeMap<String, u32>,
    groups: &mut BTreeMap<String, u32>,
    name: &str,
) {
    increment(counts, name, true);
    increment(
        counts,
        "group:pistons",
        matches!(name, "minecraft:piston" | "minecraft:sticky_piston"),
    );
    for (group_name, group) in &config.groups {
        if group.kind == GroupKind::Block && group.members.iter().any(|id| id == name) {
            *groups.entry(group_name.clone()).or_default() += 1;
        }
    }
}

pub fn status_color(count: u32, limit: u32) -> u32 {
    if count >= limit {
        0xffff_5555
    } else if u64::from(count) * 4 >= u64::from(limit) * 3 {
        0xffff_ff55
    } else {
        0xff55_ff55
    }
}

fn contains(min: [i32; 2], size: i32, chunk: [i32; 2]) -> bool {
    chunk[0] >= min[0] && chunk[0] < min[0] + size && chunk[1] >= min[1] && chunk[1] < min[1] + size
}

fn zeroed(limits: &BTreeMap<String, u32>) -> BTreeMap<String, u32> {
    limits.keys().map(|id| (id.clone(), 0)).collect()
}

fn increment(counts: &mut BTreeMap<String, u32>, id: &str, matches: bool) {
    if matches && let Some(count) = counts.get_mut(id) {
        *count += 1;
    }
}

fn overloaded(counts: &BTreeMap<String, u32>, limits: &BTreeMap<String, u32>, id: &str) -> bool {
    limits
        .get(id)
        .is_some_and(|limit| counts.get(id).copied().unwrap_or(0) > *limit)
}

fn overloaded_if(
    counts: &BTreeMap<String, u32>,
    limits: &BTreeMap<String, u32>,
    id: &str,
    matches: bool,
) -> bool {
    matches && overloaded(counts, limits, id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Group, GroupKind};

    #[test]
    fn entities_are_limited_to_the_selected_chunk_square() {
        let mut config = Config::default();
        config.entities.clear();
        config.entities.insert("minecraft:cow".into(), 1);
        config.entities.insert("group:mobs".into(), 1);
        let samples = [
            EntitySample {
                id: 1,
                kind: "minecraft:cow",
                chunk: [4, 7],
                mob: true,
                item: false,
                minecart: false,
            },
            EntitySample {
                id: 2,
                kind: "minecraft:cow",
                chunk: [5, 7],
                mob: true,
                item: false,
                minecart: false,
            },
            EntitySample {
                id: 3,
                kind: "minecraft:cow",
                chunk: [6, 7],
                mob: true,
                item: false,
                minecart: false,
            },
        ];

        let (counts, _, highlighted) = count_entities(&config, [4, 7], 2, samples);

        assert_eq!(counts["minecraft:cow"], 2);
        assert_eq!(counts["group:mobs"], 2);
        assert_eq!(highlighted, BTreeSet::from([1, 2]));
    }

    #[test]
    fn custom_groups_share_one_limit() {
        let mut config = Config::default();
        config.entities.clear();
        config.groups.clear();
        config.groups.insert(
            "animals".into(),
            Group {
                kind: GroupKind::Entity,
                limit: 1,
                members: vec!["minecraft:cow".into(), "minecraft:sheep".into()],
            },
        );
        let samples = [
            EntitySample {
                id: 10,
                kind: "minecraft:cow",
                chunk: [0, 0],
                mob: true,
                item: false,
                minecart: false,
            },
            EntitySample {
                id: 11,
                kind: "minecraft:sheep",
                chunk: [0, 0],
                mob: true,
                item: false,
                minecart: false,
            },
        ];

        let (_, groups, highlighted) = count_entities(&config, [0, 0], 1, samples);

        assert_eq!(groups["animals"], 2);
        assert_eq!(highlighted, BTreeSet::from([10, 11]));
    }

    #[test]
    fn piston_group_counts_both_variants() {
        let config = Config::default();
        let mut counts = config.blocks.keys().map(|id| (id.clone(), 0)).collect();
        let mut groups = BTreeMap::new();
        count_block(&config, &mut counts, &mut groups, "minecraft:piston");
        count_block(&config, &mut counts, &mut groups, "minecraft:sticky_piston");
        assert_eq!(counts["group:pistons"], 2);
    }

    #[test]
    fn status_turns_yellow_at_three_quarters_and_red_at_limit() {
        assert_eq!(status_color(7, 10), 0xff55_ff55);
        assert_eq!(status_color(8, 10), 0xffff_ff55);
        assert_eq!(status_color(10, 10), 0xffff_5555);
    }
}
