//! Advancement triggers - called when game events happen.

use std::sync::Arc;

use pumpkin_protocol::java::client::play::{
    AdvancementProgress, AdvancementProgressMapping, CUpdateAdvancements, CriterionProgress,
    CriterionProgressMapping,
};
use pumpkin_util::resource_location::ResourceLocation;
use pumpkin_world::item::ItemStack;

use super::current_timestamp_millis;
use crate::entity::player::Player;

/// Item ID to (`advancement_id`, `criterion_name`) mappings for `inventory_changed` triggers.
/// This is a simplified version - vanilla uses JSON conditions.
const ITEM_CRITERIA: &[(u16, &str, &str)] = &[
    // Story advancements
    (332, "minecraft:story/root", "crafting_table"),             // crafting_table
    (35, "minecraft:story/mine_stone", "get_cobblestone"),       // cobblestone
    (923, "minecraft:story/upgrade_tools", "has_stone_pickaxe"), // stone_pickaxe
    (904, "minecraft:story/smelt_iron", "has_iron_ingot"),       // iron_ingot
];

/// Checks if picking up an item should trigger any advancement criteria.
/// Called after an item is added to player inventory.
pub async fn on_inventory_changed(player: &Arc<Player>, item: &ItemStack) {
    let item_id = item.item.id;

    // Find matching criteria
    for (check_id, advancement_id, criterion_name) in ITEM_CRITERIA {
        if item_id == *check_id {
            grant_criterion(player, advancement_id, criterion_name).await;
        }
    }
}

/// Grants a criterion and sends the progress update to the client.
async fn grant_criterion(player: &Arc<Player>, advancement_id: &str, criterion_name: &str) {
    let advancement_loc = ResourceLocation::from(advancement_id);

    let criterion = CriterionProgressMapping {
        criterion: criterion_name,
        progress: CriterionProgress {
            obtained_time: Some(current_timestamp_millis()),
        },
    };

    let criteria = [criterion];
    let progress = AdvancementProgress {
        criteria: &criteria,
    };

    let progress_mapping = AdvancementProgressMapping {
        id: advancement_loc,
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
