//! Advancement triggers - called when game events happen.

use std::collections::HashMap;
use std::sync::{Arc, OnceLock, RwLock};

use pumpkin_data::item::Item;
use pumpkin_data::Block;
use pumpkin_protocol::java::client::play::{
    AdvancementProgress, AdvancementProgressMapping, CUpdateAdvancements, CriterionProgress,
    CriterionProgressMapping,
};
use pumpkin_util::resource_location::ResourceLocation;
use pumpkin_world::item::ItemStack;

use super::AdvancementRegistry;
use crate::entity::player::Player;

/// Global mapping of item ID -> list of (advancement_id, criterion_name).
/// Built from the loaded advancement registry.
static ITEM_CRITERIA_MAP: OnceLock<RwLock<HashMap<u16, Vec<(ResourceLocation, String)>>>> =
    OnceLock::new();

/// Global mapping of (from_dimension, to_dimension) -> list of (advancement_id, criterion_name).
/// For changed_dimension triggers.
static DIMENSION_CRITERIA_MAP: OnceLock<RwLock<HashMap<(Option<String>, Option<String>), Vec<(ResourceLocation, String)>>>> =
    OnceLock::new();

/// Global mapping of item ID -> list of (advancement_id, criterion_name).
/// For consume_item triggers (eating/drinking).
static CONSUME_CRITERIA_MAP: OnceLock<RwLock<HashMap<u16, Vec<(ResourceLocation, String)>>>> =
    OnceLock::new();

/// Global mapping of entity type -> list of (advancement_id, criterion_name).
/// For player_killed_entity triggers.
static KILL_ENTITY_CRITERIA_MAP: OnceLock<RwLock<HashMap<String, Vec<(ResourceLocation, String)>>>> =
    OnceLock::new();

/// Global list of recipe_unlocked criteria: (recipe_id, advancement_id, criterion_name).
static RECIPE_CRITERIA_MAP: OnceLock<RwLock<HashMap<String, Vec<(ResourceLocation, String)>>>> =
    OnceLock::new();

/// Global list of tick criteria: list of (advancement_id, criterion_name).
/// Tick triggers have no conditions and fire on every server tick.
static TICK_CRITERIA_MAP: OnceLock<RwLock<Vec<(ResourceLocation, String)>>> = OnceLock::new();

/// Global mapping of biome -> list of (advancement_id, criterion_name).
/// For location triggers that check player biome.
static LOCATION_BIOME_CRITERIA_MAP: OnceLock<RwLock<HashMap<String, Vec<(ResourceLocation, String)>>>> =
    OnceLock::new();

/// Global mapping of block ID -> list of (advancement_id, criterion_name).
/// For placed_block triggers.
static PLACED_BLOCK_CRITERIA_MAP: OnceLock<RwLock<HashMap<u16, Vec<(ResourceLocation, String)>>>> =
    OnceLock::new();

/// Global mapping of block type ID -> list of (advancement_id, criterion_name).
/// For enter_block triggers (e.g., entering water, end gateway).
/// Uses block type ID (not state ID) to match any state of a block type.
static ENTER_BLOCK_CRITERIA_MAP: OnceLock<RwLock<HashMap<u16, Vec<(ResourceLocation, String)>>>> =
    OnceLock::new();

/// Builds all criteria maps from the advancement registry.
/// Should be called after advancements are loaded.
pub fn build_criteria_maps(registry: &AdvancementRegistry) {
    let mut item_map: HashMap<u16, Vec<(ResourceLocation, String)>> = HashMap::new();
    let mut dimension_map: HashMap<(Option<String>, Option<String>), Vec<(ResourceLocation, String)>> = HashMap::new();
    let mut consume_map: HashMap<u16, Vec<(ResourceLocation, String)>> = HashMap::new();
    let mut kill_map: HashMap<String, Vec<(ResourceLocation, String)>> = HashMap::new();
    let mut recipe_map: HashMap<String, Vec<(ResourceLocation, String)>> = HashMap::new();
    let mut tick_list: Vec<(ResourceLocation, String)> = Vec::new();
    let mut location_biome_map: HashMap<String, Vec<(ResourceLocation, String)>> = HashMap::new();
    let mut placed_block_map: HashMap<u16, Vec<(ResourceLocation, String)>> = HashMap::new();
    let mut enter_block_map: HashMap<u16, Vec<(ResourceLocation, String)>> = HashMap::new();

    for advancement in registry.all() {
        for (criterion_name, criterion_data) in &advancement.criteria {
            let trigger = criterion_data.trigger.to_string();
            let conditions = &criterion_data.conditions;
            let entry = (advancement.id.clone(), criterion_name.clone());

            match trigger.as_str() {
                "minecraft:inventory_changed" => {
                    let item_ids = extract_item_ids_from_conditions(conditions);
                    for item_id in item_ids {
                        item_map.entry(item_id).or_default().push(entry.clone());
                    }
                }
                "minecraft:changed_dimension" => {
                    let from = conditions.get("from").and_then(|v| v.as_str()).map(String::from);
                    let to = conditions.get("to").and_then(|v| v.as_str()).map(String::from);
                    dimension_map.entry((from, to)).or_default().push(entry);
                }
                "minecraft:consume_item" => {
                    let item_ids = extract_consume_item_ids(conditions);
                    for item_id in item_ids {
                        consume_map.entry(item_id).or_default().push(entry.clone());
                    }
                }
                "minecraft:player_killed_entity" => {
                    let entity_types = extract_entity_types(conditions);
                    if entity_types.is_empty() {
                        // No specific entity type - add wildcard entry
                        kill_map.entry("*".to_string()).or_default().push(entry);
                    } else {
                        for entity_type in entity_types {
                            kill_map.entry(entity_type).or_default().push(entry.clone());
                        }
                    }
                }
                "minecraft:recipe_unlocked" => {
                    if let Some(recipe) = conditions.get("recipe").and_then(|v| v.as_str()) {
                        recipe_map.entry(recipe.to_string()).or_default().push(entry);
                    }
                }
                "minecraft:tick" => {
                    // Tick triggers have no conditions - they fire on server tick
                    tick_list.push(entry);
                }
                "minecraft:location" => {
                    // Location triggers check player biome/dimension/position
                    let biomes = extract_location_biomes(conditions);
                    for biome in biomes {
                        location_biome_map.entry(biome).or_default().push(entry.clone());
                    }
                }
                "minecraft:placed_block" => {
                    // Placed block triggers check what block was placed
                    let block_ids = extract_placed_block_ids(conditions);
                    if block_ids.is_empty() {
                        // No specific block - add wildcard entry (block ID 0 as wildcard)
                        placed_block_map.entry(0).or_default().push(entry);
                    } else {
                        for block_id in block_ids {
                            placed_block_map.entry(block_id).or_default().push(entry.clone());
                        }
                    }
                }
                "minecraft:enter_block" => {
                    // Enter block triggers check what block the player entered
                    let block_ids = extract_enter_block_ids(conditions);
                    for block_id in block_ids {
                        enter_block_map.entry(block_id).or_default().push(entry.clone());
                    }
                }
                _ => {
                    // Other triggers not yet implemented
                }
            }
        }
    }

    let item_count: usize = item_map.values().map(|v| v.len()).sum();
    let dim_count: usize = dimension_map.values().map(|v| v.len()).sum();
    let consume_count: usize = consume_map.values().map(|v| v.len()).sum();
    let kill_count: usize = kill_map.values().map(|v| v.len()).sum();
    let recipe_count: usize = recipe_map.values().map(|v| v.len()).sum();
    let tick_count = tick_list.len();
    let location_count: usize = location_biome_map.values().map(|v| v.len()).sum();
    let placed_block_count: usize = placed_block_map.values().map(|v| v.len()).sum();
    let enter_block_count: usize = enter_block_map.values().map(|v| v.len()).sum();

    log::info!(
        "Built advancement trigger maps: inventory_changed={}, changed_dimension={}, consume_item={}, player_killed_entity={}, recipe_unlocked={}, tick={}, location={}, placed_block={}, enter_block={}",
        item_count, dim_count, consume_count, kill_count, recipe_count, tick_count, location_count, placed_block_count, enter_block_count
    );

    let _ = ITEM_CRITERIA_MAP.set(RwLock::new(item_map));
    let _ = DIMENSION_CRITERIA_MAP.set(RwLock::new(dimension_map));
    let _ = CONSUME_CRITERIA_MAP.set(RwLock::new(consume_map));
    let _ = KILL_ENTITY_CRITERIA_MAP.set(RwLock::new(kill_map));
    let _ = RECIPE_CRITERIA_MAP.set(RwLock::new(recipe_map));
    let _ = TICK_CRITERIA_MAP.set(RwLock::new(tick_list));
    let _ = LOCATION_BIOME_CRITERIA_MAP.set(RwLock::new(location_biome_map));
    let _ = PLACED_BLOCK_CRITERIA_MAP.set(RwLock::new(placed_block_map));
    let _ = ENTER_BLOCK_CRITERIA_MAP.set(RwLock::new(enter_block_map));
}

/// Backwards compatibility alias
pub fn build_item_criteria_map(registry: &AdvancementRegistry) {
    build_criteria_maps(registry);
}

/// Gets a reference to the item criteria map.
fn get_item_criteria_map() -> &'static RwLock<HashMap<u16, Vec<(ResourceLocation, String)>>> {
    ITEM_CRITERIA_MAP.get_or_init(|| RwLock::new(HashMap::new()))
}

/// Extracts item IDs from inventory_changed trigger conditions.
fn extract_item_ids_from_conditions(conditions: &serde_json::Value) -> Vec<u16> {
    let mut item_ids = Vec::new();

    // conditions.items is an array of item predicates
    if let Some(items_array) = conditions.get("items").and_then(|v| v.as_array()) {
        for item_predicate in items_array {
            // Each predicate can have "items" (single item or tag)
            if let Some(items_value) = item_predicate.get("items") {
                match items_value {
                    serde_json::Value::String(s) => {
                        if let Some(id) = resolve_item_string(s) {
                            item_ids.push(id);
                        }
                    }
                    serde_json::Value::Array(arr) => {
                        for item in arr {
                            if let Some(s) = item.as_str() {
                                if let Some(id) = resolve_item_string(s) {
                                    item_ids.push(id);
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    item_ids
}

/// Resolves an item string to its numeric ID.
/// Handles both direct items ("minecraft:crafting_table") and tags ("#minecraft:stone_tool_materials").
fn resolve_item_string(s: &str) -> Option<u16> {
    if s.starts_with('#') {
        // It's a tag - for now, resolve common tags manually
        // TODO: Load tags from data files
        resolve_item_tag(&s[1..])
    } else {
        // Direct item reference
        let name = s.strip_prefix("minecraft:").unwrap_or(s);
        Item::from_registry_key(name).map(|item| item.id)
    }
}

/// Resolves a tag to item IDs.
/// Currently handles common tags manually - could be extended to load from data files.
fn resolve_item_tag(tag: &str) -> Option<u16> {
    // Return just one representative item for each tag for now
    // A full implementation would return all items in the tag
    match tag {
        "minecraft:stone_tool_materials" => Item::from_registry_key("cobblestone").map(|i| i.id),
        "minecraft:logs" => Item::from_registry_key("oak_log").map(|i| i.id),
        "minecraft:planks" => Item::from_registry_key("oak_planks").map(|i| i.id),
        "minecraft:coals" => Item::from_registry_key("coal").map(|i| i.id),
        "minecraft:iron_ores" => Item::from_registry_key("iron_ore").map(|i| i.id),
        "minecraft:gold_ores" => Item::from_registry_key("gold_ore").map(|i| i.id),
        "minecraft:diamond_ores" => Item::from_registry_key("diamond_ore").map(|i| i.id),
        _ => {
            log::debug!("Unknown item tag: {tag}");
            None
        }
    }
}

/// Extracts item IDs from consume_item trigger conditions.
fn extract_consume_item_ids(conditions: &serde_json::Value) -> Vec<u16> {
    let mut item_ids = Vec::new();

    // consume_item has "item" field with item predicate
    if let Some(item_predicate) = conditions.get("item") {
        if let Some(items_value) = item_predicate.get("items") {
            match items_value {
                serde_json::Value::String(s) => {
                    if let Some(id) = resolve_item_string(s) {
                        item_ids.push(id);
                    }
                }
                serde_json::Value::Array(arr) => {
                    for item in arr {
                        if let Some(s) = item.as_str() {
                            if let Some(id) = resolve_item_string(s) {
                                item_ids.push(id);
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }

    item_ids
}

/// Extracts entity types from player_killed_entity conditions.
fn extract_entity_types(conditions: &serde_json::Value) -> Vec<String> {
    let mut types = Vec::new();

    // player_killed_entity has "entity" field with entity predicate
    if let Some(entity) = conditions.get("entity") {
        if let Some(type_val) = entity.get("type") {
            if let Some(s) = type_val.as_str() {
                types.push(s.to_string());
            }
        }
    }

    types
}

/// Extracts biome identifiers from location trigger conditions.
/// Location triggers use player conditions with entity_properties predicates.
fn extract_location_biomes(conditions: &serde_json::Value) -> Vec<String> {
    let mut biomes = Vec::new();

    // Location trigger has "player" array with entity_properties conditions
    if let Some(player_array) = conditions.get("player").and_then(|v| v.as_array()) {
        for condition in player_array {
            // Check if this is an entity_properties condition
            if condition.get("condition").and_then(|v| v.as_str()) == Some("minecraft:entity_properties") {
                // Look for predicate.location.biomes
                if let Some(predicate) = condition.get("predicate") {
                    if let Some(location) = predicate.get("location") {
                        if let Some(biome) = location.get("biomes").and_then(|v| v.as_str()) {
                            biomes.push(biome.to_string());
                        }
                    }
                }
            }
        }
    }

    biomes
}

/// Extracts block IDs from placed_block trigger conditions.
fn extract_placed_block_ids(conditions: &serde_json::Value) -> Vec<u16> {
    let mut block_ids = Vec::new();

    // placed_block has "location" array with block predicates
    if let Some(location_array) = conditions.get("location").and_then(|v| v.as_array()) {
        for condition in location_array {
            // Direct block field (e.g., "block": "minecraft:creaking_heart")
            if let Some(block) = condition.get("block").and_then(|v| v.as_str()) {
                if let Some(id) = resolve_block_string(block) {
                    block_ids.push(id);
                }
            }
        }
    }

    block_ids
}

/// Extracts block type IDs from enter_block trigger conditions.
/// Uses block type ID (not state ID) since blocks like water have many states.
fn extract_enter_block_ids(conditions: &serde_json::Value) -> Vec<u16> {
    let mut block_ids = Vec::new();

    // enter_block has a simple "block" field
    if let Some(block) = conditions.get("block").and_then(|v| v.as_str()) {
        if let Some(id) = resolve_block_type_id(block) {
            block_ids.push(id);
        }
    }

    block_ids
}

/// Resolves a block string to its state ID (for placed_block which needs specific state).
fn resolve_block_string(s: &str) -> Option<u16> {
    if s.starts_with('#') {
        // It's a tag - skip for now
        log::debug!("Block tag not yet supported: {s}");
        None
    } else {
        // Direct block reference
        let name = s.strip_prefix("minecraft:").unwrap_or(s);
        Block::from_registry_key(name).map(|block| block.default_state.id)
    }
}

/// Resolves a block string to its block type ID (not state ID).
/// Used for enter_block where we need to match any state of a block type.
fn resolve_block_type_id(s: &str) -> Option<u16> {
    if s.starts_with('#') {
        // It's a tag - skip for now
        log::debug!("Block tag not yet supported: {s}");
        None
    } else {
        // Direct block reference - return block type ID
        let name = s.strip_prefix("minecraft:").unwrap_or(s);
        Block::from_registry_key(name).map(|block| block.id)
    }
}

fn get_dimension_criteria_map() -> &'static RwLock<HashMap<(Option<String>, Option<String>), Vec<(ResourceLocation, String)>>> {
    DIMENSION_CRITERIA_MAP.get_or_init(|| RwLock::new(HashMap::new()))
}

fn get_consume_criteria_map() -> &'static RwLock<HashMap<u16, Vec<(ResourceLocation, String)>>> {
    CONSUME_CRITERIA_MAP.get_or_init(|| RwLock::new(HashMap::new()))
}

fn get_kill_entity_criteria_map() -> &'static RwLock<HashMap<String, Vec<(ResourceLocation, String)>>> {
    KILL_ENTITY_CRITERIA_MAP.get_or_init(|| RwLock::new(HashMap::new()))
}

fn get_recipe_criteria_map() -> &'static RwLock<HashMap<String, Vec<(ResourceLocation, String)>>> {
    RECIPE_CRITERIA_MAP.get_or_init(|| RwLock::new(HashMap::new()))
}

fn get_tick_criteria_map() -> &'static RwLock<Vec<(ResourceLocation, String)>> {
    TICK_CRITERIA_MAP.get_or_init(|| RwLock::new(Vec::new()))
}

fn get_location_biome_criteria_map() -> &'static RwLock<HashMap<String, Vec<(ResourceLocation, String)>>> {
    LOCATION_BIOME_CRITERIA_MAP.get_or_init(|| RwLock::new(HashMap::new()))
}

fn get_placed_block_criteria_map() -> &'static RwLock<HashMap<u16, Vec<(ResourceLocation, String)>>> {
    PLACED_BLOCK_CRITERIA_MAP.get_or_init(|| RwLock::new(HashMap::new()))
}

fn get_enter_block_criteria_map() -> &'static RwLock<HashMap<u16, Vec<(ResourceLocation, String)>>> {
    ENTER_BLOCK_CRITERIA_MAP.get_or_init(|| RwLock::new(HashMap::new()))
}

/// Checks if picking up an item should trigger any advancement criteria.
/// Called after an item is added to player inventory.
pub async fn on_inventory_changed(player: &Arc<Player>, item: &ItemStack) {
    let item_id = item.item.id;

    // Clone the criteria before any await points (RwLockReadGuard is not Send)
    let criteria = {
        let map = get_item_criteria_map().read().unwrap();
        map.get(&item_id).cloned()
    };

    if let Some(criteria) = criteria {
        for (advancement_id, criterion_name) in criteria {
            grant_criterion(player, &advancement_id, &criterion_name).await;
        }
    }
}

/// Scans the player's inventory and triggers advancements for any matching items.
/// Called after inventory operations like crafting or slot clicks.
pub async fn check_inventory_for_advancements(player: &Arc<Player>) {
    let inventory = player.inventory();

    // Collect all item IDs from main inventory (includes hotbar - 36 slots total)
    let mut item_ids: Vec<u16> = Vec::new();
    for slot in inventory.main_inventory.iter() {
        let id = {
            let item = slot.lock().await;
            if item.is_empty() {
                None
            } else {
                Some(item.item.id)
            }
        };
        if let Some(id) = id {
            item_ids.push(id);
        }
    }

    // Also check offhand
    {
        let offhand = inventory.off_hand_item().await;
        let item = offhand.lock().await;
        if !item.is_empty() {
            item_ids.push(item.item.id);
        }
    }

    // Look up criteria for each item
    let items_to_check: Vec<Vec<(ResourceLocation, String)>> = {
        let map = get_item_criteria_map().read().unwrap();
        item_ids
            .iter()
            .filter_map(|id| map.get(id).cloned())
            .collect()
    };

    // Grant criteria without holding any locks
    for criteria in items_to_check {
        for (advancement_id, criterion_name) in criteria {
            grant_criterion(player, &advancement_id, &criterion_name).await;
        }
    }
}

/// Grants a criterion and sends the progress update to the client.
///
/// This function:
/// 1. Updates the player's advancement tracker
/// 2. Only sends a packet if the criterion was newly granted (prevents duplicate toasts)
/// 3. Saves progress to disk when an advancement is completed
async fn grant_criterion(player: &Arc<Player>, advancement_id: &ResourceLocation, criterion_name: &str) {
    // Grant the criterion in the player's tracker
    // This returns true only if the criterion was newly granted
    let (newly_granted, is_complete) = {
        let mut tracker = player.advancement_tracker.write().await;
        let granted = tracker.grant_criterion(advancement_id, criterion_name);
        let complete = granted && tracker.is_complete(advancement_id);
        (granted, complete)
    };

    // Only send packet if the criterion was newly granted
    // This prevents duplicate toasts on repeated triggers
    if !newly_granted {
        return;
    }

    // Save progress when an advancement is completed
    if is_complete {
        super::save_advancements(player).await;
    }

    // Get the obtained time from the tracker
    let obtained_time = {
        let tracker = player.advancement_tracker.read().await;
        tracker
            .get_progress(advancement_id)
            .and_then(|p| p.criteria.get(criterion_name).copied().flatten())
    };

    let criterion = CriterionProgressMapping {
        criterion: criterion_name,
        progress: CriterionProgress {
            obtained_time,
        },
    };

    let criteria = [criterion];
    let progress = AdvancementProgress {
        criteria: &criteria,
    };

    let progress_mapping = AdvancementProgressMapping {
        id: advancement_id.clone(),
        progress,
    };

    let progress_mappings = [progress_mapping];

    // Send incremental update (reset=false, no new advancements, just progress)
    let packet = CUpdateAdvancements::new(
        false,        // Don't reset
        &[],          // No new advancements
        &[],          // No removals
        &progress_mappings,
        true,         // Show toast
    );

    player.client.enqueue_packet(&packet).await;
}

/// Triggers advancement criteria when player changes dimension.
/// Called when a player enters a portal or teleports between dimensions.
///
/// # Arguments
/// * `player` - The player who changed dimension
/// * `from` - The dimension the player came from (e.g., "minecraft:overworld")
/// * `to` - The dimension the player went to (e.g., "minecraft:the_nether")
pub async fn on_changed_dimension(player: &Arc<Player>, from: &str, to: &str) {
    let criteria = {
        let map = get_dimension_criteria_map().read().unwrap();
        let mut results = Vec::new();

        // Check for exact match (from, to)
        if let Some(c) = map.get(&(Some(from.to_string()), Some(to.to_string()))) {
            results.extend(c.clone());
        }
        // Check for wildcard from (None, to)
        if let Some(c) = map.get(&(None, Some(to.to_string()))) {
            results.extend(c.clone());
        }
        // Check for wildcard to (from, None)
        if let Some(c) = map.get(&(Some(from.to_string()), None)) {
            results.extend(c.clone());
        }
        // Check for both wildcards (None, None)
        if let Some(c) = map.get(&(None, None)) {
            results.extend(c.clone());
        }

        results
    };

    for (advancement_id, criterion_name) in criteria {
        grant_criterion(player, &advancement_id, &criterion_name).await;
    }
}

/// Triggers advancement criteria when player consumes an item (eats food or drinks potion).
/// Called after item consumption is complete.
///
/// # Arguments
/// * `player` - The player who consumed the item
/// * `item` - The item that was consumed
pub async fn on_consume_item(player: &Arc<Player>, item: &ItemStack) {
    let item_id = item.item.id;

    let criteria = {
        let map = get_consume_criteria_map().read().unwrap();
        map.get(&item_id).cloned()
    };

    if let Some(criteria) = criteria {
        for (advancement_id, criterion_name) in criteria {
            grant_criterion(player, &advancement_id, &criterion_name).await;
        }
    }
}

/// Triggers advancement criteria when player kills an entity.
/// Called after the entity death is confirmed.
///
/// # Arguments
/// * `player` - The player who killed the entity
/// * `entity_type` - The type of entity killed (e.g., "minecraft:zombie")
pub async fn on_player_killed_entity(player: &Arc<Player>, entity_type: &str) {
    let criteria = {
        let map = get_kill_entity_criteria_map().read().unwrap();
        let mut results = Vec::new();

        // Check for specific entity type
        if let Some(c) = map.get(entity_type) {
            results.extend(c.clone());
        }
        // Also check wildcard criteria (any entity)
        if let Some(c) = map.get("*") {
            results.extend(c.clone());
        }

        results
    };

    for (advancement_id, criterion_name) in criteria {
        grant_criterion(player, &advancement_id, &criterion_name).await;
    }
}

/// Triggers advancement criteria when a recipe is unlocked.
/// Called when the player unlocks a new recipe.
///
/// # Arguments
/// * `player` - The player who unlocked the recipe
/// * `recipe_id` - The ID of the unlocked recipe (e.g., "minecraft:crafting_table")
pub async fn on_recipe_unlocked(player: &Arc<Player>, recipe_id: &str) {
    let criteria = {
        let map = get_recipe_criteria_map().read().unwrap();
        map.get(recipe_id).cloned()
    };

    if let Some(criteria) = criteria {
        for (advancement_id, criterion_name) in criteria {
            grant_criterion(player, &advancement_id, &criterion_name).await;
        }
    }
}

/// Triggers all tick-based advancement criteria.
/// Called once per server tick for each player.
/// This is used for advancements that should be granted immediately (e.g., recipe unlocks).
///
/// # Arguments
/// * `player` - The player to check tick criteria for
pub async fn on_tick(player: &Arc<Player>) {
    let criteria = {
        let list = get_tick_criteria_map().read().unwrap();
        list.clone()
    };

    for (advancement_id, criterion_name) in criteria {
        grant_criterion(player, &advancement_id, &criterion_name).await;
    }
}

/// Triggers location-based advancement criteria.
/// Called every 20 ticks to check player location conditions.
/// Matches vanilla behavior from ServerPlayerEntity.java:717-718
///
/// # Arguments
/// * `player` - The player to check location for
pub async fn on_location(player: &Arc<Player>) {
    // Get player's current position and biome
    let pos = player.living_entity.entity.pos.load();
    let block_pos = pumpkin_util::math::position::BlockPos(
        pumpkin_util::math::vector3::Vector3::new(
            pos.x.floor() as i32,
            pos.y.floor() as i32,
            pos.z.floor() as i32,
        ),
    );

    let world = player.world();
    let biome = world.level.get_rough_biome(&block_pos).await;
    let biome_id = format!("minecraft:{}", biome.registry_id);

    // Check biome-based location criteria
    let criteria = {
        let map = get_location_biome_criteria_map().read().unwrap();
        map.get(&biome_id).cloned()
    };

    if let Some(criteria) = criteria {
        for (advancement_id, criterion_name) in criteria {
            grant_criterion(player, &advancement_id, &criterion_name).await;
        }
    }

    // TODO: Check other location conditions (equipment, position predicates, etc.)
    // Vanilla evaluates full LootContextPredicate which includes:
    // - Entity position
    // - Equipment (e.g., boots on specific block)
    // - Effects
    // - Dimension
    // For now, biome-only checks cover "Adventuring Time" advancement
}

/// Triggers advancement criteria when player places a block.
/// Called after a block is successfully placed.
///
/// # Arguments
/// * `player` - The player who placed the block
/// * `block_state_id` - The state ID of the placed block
pub async fn on_placed_block(player: &Arc<Player>, block_state_id: u16) {
    let criteria = {
        let map = get_placed_block_criteria_map().read().unwrap();
        let mut results = Vec::new();

        // Check for specific block
        if let Some(c) = map.get(&block_state_id) {
            results.extend(c.clone());
        }
        // Also check wildcard criteria (any block, using ID 0)
        if let Some(c) = map.get(&0) {
            results.extend(c.clone());
        }

        results
    };

    for (advancement_id, criterion_name) in criteria {
        grant_criterion(player, &advancement_id, &criterion_name).await;
    }
}

/// Triggers advancement criteria when player enters a block.
/// Called when a player moves into a block (e.g., water, end gateway, cobweb).
/// Matches vanilla behavior from ServerPlayerEntity.onBlockCollision(BlockState state)
///
/// # Arguments
/// * `player` - The player who entered the block
/// * `block_state_id` - The state ID of the block the player entered
pub async fn on_enter_block(player: &Arc<Player>, block_state_id: u16) {
    // Convert state ID to block type ID for lookup
    // This handles blocks with multiple states (like water with different levels)
    let block = Block::from_state_id(block_state_id);
    let block_type_id = block.id;

    let criteria = {
        let map = get_enter_block_criteria_map().read().unwrap();
        map.get(&block_type_id).cloned()
    };

    if let Some(criteria) = criteria {
        for (advancement_id, criterion_name) in criteria {
            grant_criterion(player, &advancement_id, &criterion_name).await;
        }
    }
}
