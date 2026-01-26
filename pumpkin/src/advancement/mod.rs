//! Advancement system for Pumpkin.
//!
//! This module implements the vanilla-compatible advancement system, allowing
//! players to view their advancement progress in the advancement GUI (pressed L).

use std::borrow::Cow;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use pumpkin_data::item::Item;
use pumpkin_protocol::codec::item_stack_seralizer::ItemStackSerializer;
use pumpkin_protocol::java::client::play::{
    Advancement, AdvancementDisplay, AdvancementFrameType, AdvancementMapping,
    AdvancementProgress, AdvancementProgressMapping, CUpdateAdvancements, CriterionProgress,
    CriterionProgressMapping,
};
use pumpkin_util::resource_location::ResourceLocation;
use pumpkin_util::text::TextComponent;
use pumpkin_world::item::ItemStack;

use crate::entity::player::Player;

/// Gets the current timestamp in milliseconds since Unix epoch.
fn current_timestamp_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Display flags for advancements.
mod flags {
    /// Advancement has a background texture.
    pub const HAS_BACKGROUND: i32 = 0x1;
    /// Show toast notification when completed.
    pub const SHOW_TOAST: i32 = 0x2;
    /// Hidden until completed or has completed child.
    #[allow(dead_code)]
    pub const HIDDEN: i32 = 0x4;
}

/// Creates an item stack for the given item ID.
fn create_item_stack(item_id: u16) -> ItemStack {
    ItemStack::new(1, Item::from_id(item_id).unwrap_or(&Item::AIR))
}

/// Sends the initial advancements to a player when they join.
///
/// TODO: Once triggers are implemented, this should send an empty packet initially
/// (showing "There doesn't seem to be anything here") and only send advancements
/// when the player actually earns them. For now, we send test advancements with
/// the root granted so we can verify the visual system works.
#[expect(clippy::too_many_lines)]
pub async fn send_advancements(player: &Arc<Player>) {
    // Text components for Story tab
    let story_root_title = TextComponent::text("Minecraft");
    let story_root_desc = TextComponent::text("The heart and story of the game");
    let stone_age_title = TextComponent::text("Stone Age");
    let stone_age_desc = TextComponent::text("Mine Stone with your new Pickaxe");
    let upgrade_title = TextComponent::text("Getting an Upgrade");
    let upgrade_desc = TextComponent::text("Construct a better Pickaxe");
    let iron_title = TextComponent::text("Acquire Hardware");
    let iron_desc = TextComponent::text("Smelt an Iron Ingot");

    // Item stacks for icons
    let grass_stack = create_item_stack(197); // Grass block
    let cobble_stack = create_item_stack(14); // Cobblestone
    let stone_pick_stack = create_item_stack(880); // Stone pickaxe
    let iron_stack = create_item_stack(851); // Iron ingot

    let grass_icon = ItemStackSerializer(Cow::Borrowed(&grass_stack));
    let cobble_icon = ItemStackSerializer(Cow::Borrowed(&cobble_stack));
    let stone_pick_icon = ItemStackSerializer(Cow::Borrowed(&stone_pick_stack));
    let iron_icon = ItemStackSerializer(Cow::Borrowed(&iron_stack));

    // Story root display (has background)
    let story_root_display = AdvancementDisplay {
        title: &story_root_title,
        description: &story_root_desc,
        icon: &grass_icon,
        frame_type: AdvancementFrameType::Task,
        flags: flags::HAS_BACKGROUND | flags::SHOW_TOAST,
        background_texture: Some(ResourceLocation::from(
            "minecraft:gui/advancements/backgrounds/stone",
        )),
        x: 0.0,
        y: 0.0,
    };

    // Child advancement displays
    let stone_age_display = AdvancementDisplay {
        title: &stone_age_title,
        description: &stone_age_desc,
        icon: &cobble_icon,
        frame_type: AdvancementFrameType::Task,
        flags: flags::SHOW_TOAST,
        background_texture: None,
        x: 2.0,
        y: 0.0,
    };

    let upgrade_display = AdvancementDisplay {
        title: &upgrade_title,
        description: &upgrade_desc,
        icon: &stone_pick_icon,
        frame_type: AdvancementFrameType::Task,
        flags: flags::SHOW_TOAST,
        background_texture: None,
        x: 4.0,
        y: 0.0,
    };

    let iron_display = AdvancementDisplay {
        title: &iron_title,
        description: &iron_desc,
        icon: &iron_icon,
        frame_type: AdvancementFrameType::Task,
        flags: flags::SHOW_TOAST,
        background_texture: None,
        x: 6.0,
        y: 0.0,
    };

    // Requirements
    let root_req: &[&[&str]] = &[&["crafting_table"]];
    let stone_req: &[&[&str]] = &[&["get_cobblestone"]];
    let upgrade_req: &[&[&str]] = &[&["has_stone_pickaxe"]];
    let iron_req: &[&[&str]] = &[&["has_iron_ingot"]];

    // Build advancement mappings
    let advancements = [
        AdvancementMapping {
            id: ResourceLocation::from("minecraft:story/root"),
            advancement: Advancement {
                parent: None,
                display: Some(story_root_display),
                requirements: root_req,
                sends_telemetry_event: false,
            },
        },
        AdvancementMapping {
            id: ResourceLocation::from("minecraft:story/mine_stone"),
            advancement: Advancement {
                parent: Some(ResourceLocation::from("minecraft:story/root")),
                display: Some(stone_age_display),
                requirements: stone_req,
                sends_telemetry_event: false,
            },
        },
        AdvancementMapping {
            id: ResourceLocation::from("minecraft:story/upgrade_tools"),
            advancement: Advancement {
                parent: Some(ResourceLocation::from("minecraft:story/mine_stone")),
                display: Some(upgrade_display),
                requirements: upgrade_req,
                sends_telemetry_event: false,
            },
        },
        AdvancementMapping {
            id: ResourceLocation::from("minecraft:story/smelt_iron"),
            advancement: Advancement {
                parent: Some(ResourceLocation::from("minecraft:story/upgrade_tools")),
                display: Some(iron_display),
                requirements: iron_req,
                sends_telemetry_event: false,
            },
        },
    ];

    // Grant root advancement so the tab appears
    let root_criterion = CriterionProgressMapping {
        criterion: "crafting_table",
        progress: CriterionProgress {
            obtained_time: Some(current_timestamp_millis()),
        },
    };

    let root_progress = AdvancementProgress {
        criteria: &[root_criterion],
    };

    let progress = [AdvancementProgressMapping {
        id: ResourceLocation::from("minecraft:story/root"),
        progress: root_progress,
    }];

    let packet = CUpdateAdvancements::new(
        true,
        &advancements,
        &[],
        &progress,
        true,
    );

    player.client.enqueue_packet(&packet).await;
}
