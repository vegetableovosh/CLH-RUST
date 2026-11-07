use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fs, path::Path};

pub const MIN_AREA: i32 = 1;
pub const MAX_AREA: i32 = 32;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default)]
pub struct Config {
    pub enabled: bool,
    pub hud_enabled: bool,
    pub update_interval_ticks: u32,
    pub x: i32,
    pub y: i32,
    pub hud_scale: f32,
    pub chunk_area_size: i32,
    pub show_only_non_zero: bool,
    pub highlight_overloaded_entities: bool,
    pub max_highlighted_entities: usize,
    pub entities: BTreeMap<String, u32>,
    pub blocks: BTreeMap<String, u32>,
    pub groups: BTreeMap<String, Group>,
}

impl Default for Config {
    fn default() -> Self {
        let mut entities = BTreeMap::new();
        entities.insert("minecraft:villager".into(), 30);
        entities.insert("group:minecarts".into(), 8);

        let mut blocks = BTreeMap::new();
        blocks.insert("minecraft:hopper".into(), 20);
        blocks.insert("group:pistons".into(), 64);
        blocks.insert("minecraft:observer".into(), 64);

        let mut groups = BTreeMap::new();
        groups.insert(
            "хранилища".into(),
            Group {
                kind: GroupKind::Block,
                limit: 64,
                members: vec![
                    "minecraft:chest".into(),
                    "minecraft:trapped_chest".into(),
                    "minecraft:barrel".into(),
                    "minecraft:shulker_box".into(),
                ],
            },
        );

        Self {
            enabled: true,
            hud_enabled: true,
            update_interval_ticks: 20,
            x: 8,
            y: 8,
            hud_scale: 1.0,
            chunk_area_size: 1,
            show_only_non_zero: false,
            highlight_overloaded_entities: true,
            max_highlighted_entities: 64,
            entities,
            blocks,
            groups,
        }
    }
}

impl Config {
    pub fn load(path: &Path) -> Self {
        let mut config: Self = fs::read_to_string(path)
            .ok()
            .and_then(|json| serde_json::from_str(&json).ok())
            .unwrap_or_default();
        config.sanitize();
        let _ = config.save(path);
        config
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        let json = serde_json::to_string_pretty(self).map_err(|error| error.to_string())?;
        fs::write(path, json).map_err(|error| error.to_string())
    }

    pub fn sanitize(&mut self) {
        self.update_interval_ticks = self.update_interval_ticks.max(1);
        self.x = self.x.max(0);
        self.y = self.y.max(0);
        if !self.hud_scale.is_finite() {
            self.hud_scale = 1.0;
        }
        self.hud_scale = self.hud_scale.clamp(0.5, 2.0);
        self.chunk_area_size = self.chunk_area_size.clamp(MIN_AREA, MAX_AREA);
        self.max_highlighted_entities = self.max_highlighted_entities.clamp(1, 512);
        self.entities.retain(|id, limit| valid_id(id) && *limit > 0);
        self.blocks.retain(|id, limit| valid_id(id) && *limit > 0);
        self.groups.retain(|name, group| {
            group.limit = group.limit.max(1);
            group.members.retain(|id| valid_id(id));
            !name.trim().is_empty()
        });
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum GroupKind {
    Block,
    Entity,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Group {
    #[serde(rename = "type")]
    pub kind: GroupKind,
    pub limit: u32,
    pub members: Vec<String>,
}

fn valid_id(id: &str) -> bool {
    let Some((namespace, path)) = id.split_once(':') else {
        return false;
    };
    !namespace.is_empty()
        && !path.is_empty()
        && namespace.bytes().all(|c| {
            c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, b'_' | b'-' | b'.')
        })
        && path.bytes().all(|c| {
            c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, b'_' | b'-' | b'.' | b'/')
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_clamps_values_and_drops_invalid_limits() {
        let mut config = Config {
            chunk_area_size: 100,
            hud_scale: f32::NAN,
            max_highlighted_entities: 0,
            ..Config::default()
        };
        config.entities.insert("not an id".into(), 3);
        config.blocks.insert("minecraft:stone".into(), 0);

        config.sanitize();

        assert_eq!(config.chunk_area_size, MAX_AREA);
        assert_eq!(config.hud_scale, 1.0);
        assert_eq!(config.max_highlighted_entities, 1);
        assert!(!config.entities.contains_key("not an id"));
        assert!(!config.blocks.contains_key("minecraft:stone"));
    }

    #[test]
    fn default_config_keeps_java_ports_defaults() {
        let config = Config::default();
        assert_eq!(config.entities["minecraft:villager"], 30);
        assert_eq!(config.blocks["minecraft:hopper"], 20);
        assert_eq!(config.groups["хранилища"].members.len(), 4);
    }
}
