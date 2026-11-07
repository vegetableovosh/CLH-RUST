//! CLH-RUST — a native Horizon port of Chunk Limiter HUD.
mod config;
mod counter;

use config::{Config, GroupKind};
use counter::{EntitySample, Snapshot, count_block, count_entities, status_color};
use horizon_client::hud_layout::PoseOp;
use horizon_client::mod_api::{
    ClientLevelLoadEvent, ClientLevelUnloadEvent, ClientTickPostEvent, EntityOutlineEvent, GuiDraw,
    Hooks, Host, HudEvent, InputKeyEvent, Text,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
    sync::{Mutex, mpsc},
};

const CONFIG_FILE: &str = "clh-rust.json";
const WHITE: u32 = 0xffff_ffff;
const BACKGROUND: u32 = 0xe018_191c;
const HEADER: u32 = 0xff27_292e;
const ROW: u32 = 0xe823_252a;

pub fn hooks() -> Box<dyn Hooks> {
    Box::new(Mod {
        state: Mutex::new(State::default()),
    })
}

struct Mod {
    state: Mutex<State>,
}

struct State {
    config: Config,
    config_path: Option<PathBuf>,
    snapshot: Snapshot,
    ticks_until_scan: u32,
    generation: u64,
    scan: Option<mpsc::Receiver<BlockScan>>,
}

impl Default for State {
    fn default() -> Self {
        let config = Config::default();
        Self {
            snapshot: Snapshot::empty(&config),
            config,
            config_path: None,
            ticks_until_scan: 0,
            generation: 0,
            scan: None,
        }
    }
}

struct BlockScan {
    generation: u64,
    center_chunk: [i32; 2],
    loaded_chunks: usize,
    blocks: BTreeMap<String, u32>,
    groups: BTreeMap<String, u32>,
}

impl State {
    fn ensure_config(&mut self, host: &dyn Host) {
        if self.config_path.is_some() {
            return;
        }
        let Ok(dir) = host.config_dir() else {
            return;
        };
        let path = dir.join(CONFIG_FILE);
        self.config = Config::load(&path);
        self.snapshot = Snapshot::empty(&self.config);
        self.config_path = Some(path);
    }

    fn save(&self) {
        if let Some(path) = &self.config_path {
            let _ = self.config.save(path);
        }
    }

    fn reload(&mut self) {
        if let Some(path) = &self.config_path {
            self.config = Config::load(path);
            self.snapshot = Snapshot::empty(&self.config);
            self.ticks_until_scan = 0;
            self.scan = None;
        }
    }

    fn take_finished_scan(&mut self) {
        let Some(receiver) = &self.scan else {
            return;
        };
        match receiver.try_recv() {
            Ok(result) => {
                self.scan = None;
                if result.generation != self.generation
                    || result.center_chunk != self.snapshot.center_chunk
                {
                    self.ticks_until_scan = 0;
                    return;
                }
                self.snapshot.loaded_chunks = result.loaded_chunks;
                self.snapshot.blocks = result.blocks;
                for (name, count) in result.groups {
                    self.snapshot.groups.insert(name, count);
                }
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                self.scan = None;
                self.ticks_until_scan = 0;
            }
            Err(mpsc::TryRecvError::Empty) => {}
        }
    }
}

impl Hooks for Mod {
    fn client_tick_post(&self, host: &mut dyn Host, _event: &mut ClientTickPostEvent) {
        let mut state = lock(&self.state);
        state.ensure_config(host);
        state.take_finished_scan();

        let Some(player) = host.player() else {
            state.snapshot = Snapshot::empty(&state.config);
            state.scan = None;
            return;
        };
        if !state.config.enabled {
            state.snapshot = Snapshot::empty(&state.config);
            state.scan = None;
            return;
        }

        let center = [block_coord(player.x) >> 4, block_coord(player.z) >> 4];
        let moved = center != state.snapshot.center_chunk;
        if state.ticks_until_scan > 0 {
            state.ticks_until_scan -= 1;
        }
        if state.scan.is_some() || (!moved && state.ticks_until_scan > 0) {
            return;
        }

        let config = state.config.clone();
        let min = [
            center[0] - config.chunk_area_size / 2,
            center[1] - config.chunk_area_size / 2,
        ];
        let entities = host.client_entities();
        let items = host.client_items();
        let mut samples = Vec::with_capacity(entities.len() + items.len());
        for entity in &entities {
            samples.push(EntitySample {
                id: entity.id,
                kind: &entity.kind,
                chunk: chunk_of(entity.position),
                mob: entity.player_uuid.is_none(),
                item: false,
                minecart: entity.kind.contains("minecart"),
            });
        }
        for item in &items {
            samples.push(EntitySample {
                id: item.id,
                kind: "minecraft:item",
                chunk: chunk_of(item.position),
                mob: false,
                item: true,
                minecart: false,
            });
        }
        let (entity_counts, entity_groups, highlighted) =
            count_entities(&config, min, config.chunk_area_size, samples);

        state.generation = state.generation.wrapping_add(1);
        let generation = state.generation;
        let mut snapshot = Snapshot::empty(&config);
        snapshot.center_chunk = center;
        snapshot.min_chunk = min;
        snapshot.entities = entity_counts;
        snapshot.highlighted_entities = highlighted;
        for (name, count) in entity_groups {
            snapshot.groups.insert(name, count);
        }
        state.snapshot = snapshot;
        state.ticks_until_scan = config.update_interval_ticks;

        let to = [
            min[0] + config.chunk_area_size - 1,
            min[1] + config.chunk_area_size - 1,
        ];
        let Ok(chunks) = host.client_chunks(min, to) else {
            return;
        };
        let states = host.block_states();
        let (sender, receiver) = mpsc::channel();
        state.scan = Some(receiver);
        std::thread::spawn(move || {
            let (loaded_chunks, blocks, groups) = scan_blocks(&config, &chunks, &states, min);
            let _ = sender.send(BlockScan {
                generation,
                center_chunk: center,
                loaded_chunks,
                blocks,
                groups,
            });
        });
    }

    fn hud(&self, host: &mut dyn Host, event: &mut HudEvent<'_>) {
        let state = lock(&self.state);
        if !state.config.enabled || !state.config.hud_enabled {
            return;
        }
        let Some(client) = host.client_state() else {
            return;
        };
        if client.world.is_none() {
            return;
        }
        draw_hud(host, event, &state.config, &state.snapshot);
    }

    fn input_key(&self, host: &mut dyn Host, event: &mut InputKeyEvent<'_>) {
        use horizon_client::mod_api::winit::{
            event::ElementState,
            keyboard::{KeyCode, PhysicalKey},
        };
        if event.event.state != ElementState::Pressed || event.event.repeat {
            return;
        }
        let PhysicalKey::Code(key) = event.event.physical_key else {
            return;
        };
        let mut state = lock(&self.state);
        state.ensure_config(host);
        match key {
            KeyCode::KeyO => {
                state.config.hud_enabled = !state.config.hud_enabled;
                state.save();
                let status = if state.config.hud_enabled {
                    "включён"
                } else {
                    "выключен"
                };
                host.client_action_bar(Text::literal(format!("CLH-RUST: HUD {status}")), false);
            }
            KeyCode::KeyL => {
                state.reload();
                host.client_action_bar(Text::literal("CLH-RUST: настройки перечитаны"), false);
            }
            _ => {}
        }
    }

    fn entity_outline(&self, _host: &dyn Host, event: &mut EntityOutlineEvent<'_>) {
        let state = lock(&self.state);
        if state.config.enabled
            && state.config.highlight_overloaded_entities
            && state
                .snapshot
                .highlighted_entities
                .contains(&event.entity_id)
        {
            *event.outline = Some(0xff_3030);
        }
    }

    fn client_level_load(&self, _host: &mut dyn Host, _event: &mut ClientLevelLoadEvent<'_>) {
        lock(&self.state).ticks_until_scan = 0;
    }

    fn client_level_unload(&self, _host: &mut dyn Host, _event: &mut ClientLevelUnloadEvent<'_>) {
        let mut state = lock(&self.state);
        state.snapshot = Snapshot::empty(&state.config);
        state.scan = None;
        state.ticks_until_scan = 0;
        state.generation = state.generation.wrapping_add(1);
    }
}

fn scan_blocks(
    config: &Config,
    chunks: &horizon_client::mod_api::NativeClientChunks,
    states: &horizon_client::mod_api::BlockStates,
    min: [i32; 2],
) -> (usize, BTreeMap<String, u32>, BTreeMap<String, u32>) {
    let mut counts = config.blocks.keys().map(|id| (id.clone(), 0)).collect();
    let mut groups: BTreeMap<String, u32> = config
        .groups
        .iter()
        .filter(|(_, group)| group.kind == GroupKind::Block)
        .map(|(name, _)| (name.clone(), 0))
        .collect();
    let mut targets: BTreeSet<&str> = config
        .blocks
        .keys()
        .filter(|id| !id.starts_with("group:"))
        .map(String::as_str)
        .collect();
    for group in config
        .groups
        .values()
        .filter(|group| group.kind == GroupKind::Block)
    {
        targets.extend(group.members.iter().map(String::as_str));
    }
    if config.blocks.contains_key("group:pistons") {
        targets.insert("minecraft:piston");
        targets.insert("minecraft:sticky_piston");
    }

    let mut loaded = 0;
    for chunk_z in min[1]..min[1] + config.chunk_area_size {
        for chunk_x in min[0]..min[0] + config.chunk_area_size {
            let origin = [chunk_x << 4, chunk_z << 4];
            if !chunks.loaded(origin) {
                continue;
            }
            loaded += 1;
            for dz in 0..16 {
                for dx in 0..16 {
                    let x = origin[0] + dx;
                    let z = origin[1] + dz;
                    let top = chunks.section_top([x, z]);
                    for y in chunks.height().start..top {
                        let Some(facts) = states.get(chunks.block([x, y, z])) else {
                            continue;
                        };
                        if !facts.air && targets.contains(facts.name.as_ref()) {
                            count_block(config, &mut counts, &mut groups, &facts.name);
                        }
                    }
                }
            }
        }
    }
    (loaded, counts, groups)
}

fn draw_hud(host: &dyn Host, event: &mut HudEvent<'_>, config: &Config, snapshot: &Snapshot) {
    let total = snapshot.area_size * snapshot.area_size;
    let title = format!(
        "Лимиты {}×{}  •  чанки {}/{}",
        snapshot.area_size, snapshot.area_size, snapshot.loaded_chunks, total
    );
    let mut rows = Vec::new();
    append_rows(
        &mut rows,
        &snapshot.entities,
        &config.entities,
        config.show_only_non_zero,
    );
    append_rows(
        &mut rows,
        &snapshot.blocks,
        &config.blocks,
        config.show_only_non_zero,
    );
    for (name, group) in &config.groups {
        let count = snapshot.groups.get(name).copied().unwrap_or(0);
        if !config.show_only_non_zero || count > 0 {
            rows.push(row(name, count, group.limit));
        }
    }

    let text_width = std::iter::once(title.as_str())
        .chain(rows.iter().map(|row| row.text.as_str()))
        .map(|line| host.text_width(line))
        .fold(174.0, f32::max);
    let width = text_width.ceil() as i32 + 16;
    let height = 22 + rows.len() as i32 * 14;
    let scaled_width = (width as f32 * config.hud_scale).ceil() as i32;
    let scaled_height = (height as f32 * config.hud_scale).ceil() as i32;
    let x = config
        .x
        .min((event.batch.width as i32 - scaled_width).max(0));
    let y = config
        .y
        .min((event.batch.height as i32 - scaled_height).max(0));

    let mut draws = Vec::with_capacity(6 + rows.len() * 3);
    draws.push(GuiDraw::Push(vec![
        PoseOp::Translate(x as f32, y as f32),
        PoseOp::Scale(config.hud_scale, config.hud_scale),
    ]));
    draws.push(fill(0, 0, width, height, BACKGROUND));
    draws.push(fill(1, 1, width - 1, 18, HEADER));
    draws.push(text(title, 7, 5, WHITE));
    for (index, row) in rows.into_iter().enumerate() {
        let row_y = 20 + index as i32 * 14;
        draws.push(fill(3, row_y, width - 3, row_y + 12, ROW));
        draws.push(fill(3, row_y, 5, row_y + 12, row.color));
        draws.push(text(row.text, 8, row_y + 2, row.color));
    }
    draws.push(GuiDraw::Pop);
    host.gui_paint(event.batch, &draws);
}

struct HudRow {
    text: String,
    color: u32,
}

fn append_rows(
    rows: &mut Vec<HudRow>,
    counts: &BTreeMap<String, u32>,
    limits: &BTreeMap<String, u32>,
    non_zero: bool,
) {
    for (id, limit) in limits {
        let count = counts.get(id).copied().unwrap_or(0);
        if !non_zero || count > 0 {
            rows.push(row(&display_name(id), count, *limit));
        }
    }
}

fn row(name: &str, count: u32, limit: u32) -> HudRow {
    HudRow {
        text: format!(
            "{name}: {count} / {limit}  •  осталось {}",
            limit.saturating_sub(count)
        ),
        color: status_color(count, limit),
    }
}

fn display_name(id: &str) -> String {
    match id {
        "group:entities" => "все сущности".into(),
        "group:mobs" => "все мобы".into(),
        "group:minecarts" => "все вагонетки".into(),
        "group:items" => "выброшенные предметы".into(),
        "group:pistons" => "все поршни".into(),
        _ => id
            .split_once(':')
            .map_or(id, |(_, path)| path)
            .replace('_', " "),
    }
}

fn fill(x0: i32, y0: i32, x1: i32, y1: i32, color: u32) -> GuiDraw {
    GuiDraw::Fill {
        x0,
        y0,
        x1,
        y1,
        color,
    }
}

fn text(text: impl Into<String>, x: i32, y: i32, color: u32) -> GuiDraw {
    GuiDraw::Text {
        text: Text::literal(text.into()),
        x,
        y,
        color,
        shadow: true,
    }
}

fn block_coord(value: f64) -> i32 {
    value.floor() as i32
}

fn chunk_of([x, _, z]: [f64; 3]) -> [i32; 2] {
    [block_coord(x) >> 4, block_coord(z) >> 4]
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn negative_block_positions_use_minecraft_chunks() {
        assert_eq!(chunk_of([-0.01, 64.0, -16.0]), [-1, -1]);
        assert_eq!(chunk_of([15.99, 64.0, 31.99]), [0, 1]);
    }

    #[test]
    fn display_names_cover_builtin_groups() {
        assert_eq!(display_name("group:pistons"), "все поршни");
        assert_eq!(display_name("minecraft:sticky_piston"), "sticky piston");
    }
}
