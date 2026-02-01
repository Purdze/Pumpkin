//! Advancement registry - loads and stores all advancements.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use pumpkin_util::resource_location::ResourceLocation;
use std::sync::RwLock;

use super::loader;
use super::{AdvancementData, AdvancementDisplayData, AdvancementFrame, CriterionData};

/// Global registry holding all loaded advancements.
pub struct AdvancementRegistry {
    /// All advancements by ID.
    advancements: HashMap<ResourceLocation, Arc<AdvancementData>>,
    /// Root advancements (no parent).
    roots: Vec<ResourceLocation>,
}

impl AdvancementRegistry {
    /// Creates a new empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self {
            advancements: HashMap::new(),
            roots: Vec::new(),
        }
    }

    /// Creates a registry with hardcoded test advancements.
    /// TODO: Replace with JSON loading from datapacks.
    #[must_use]
    pub fn with_test_advancements() -> Self {
        let mut registry = Self::new();
        registry.load_test_advancements();
        registry
    }

    /// Loads advancements from embedded data (compiled into the binary).
    /// This matches vanilla Minecraft where data is inside the JAR.
    pub fn load_embedded(&mut self, namespace: &str) {
        let advancements = loader::load_embedded_advancements(namespace);
        for adv in advancements {
            if adv.parent.is_none() && adv.display.is_some() {
                self.roots.push(adv.id.clone());
            }
            self.advancements.insert(adv.id.clone(), Arc::new(adv));
        }
    }

    /// Loads advancements from a data directory.
    /// Expected path: `<base_path>/<namespace>/advancement/`
    pub fn load_from_dir(&mut self, base_path: &Path, namespace: &str) {
        match loader::load_advancements_from_dir(base_path, namespace) {
            Ok(advancements) => {
                log::info!(
                    "Loaded {} advancements from {}/{}",
                    advancements.len(),
                    base_path.display(),
                    namespace
                );
                for adv in advancements {
                    if adv.parent.is_none() {
                        self.roots.push(adv.id.clone());
                    }
                    self.advancements.insert(adv.id.clone(), Arc::new(adv));
                }
            }
            Err(e) => {
                log::warn!("Failed to load advancements: {e}");
            }
        }
    }

    /// Gets an advancement by ID.
    #[must_use]
    pub fn get(&self, id: &ResourceLocation) -> Option<&Arc<AdvancementData>> {
        self.advancements.get(id)
    }

    /// Gets all advancements.
    pub fn all(&self) -> impl Iterator<Item = &Arc<AdvancementData>> {
        self.advancements.values()
    }

    /// Gets root advancements.
    #[must_use]
    pub fn roots(&self) -> &[ResourceLocation] {
        &self.roots
    }

    /// Loads test advancements (temporary until JSON loading is implemented).
    #[expect(clippy::too_many_lines)]
    fn load_test_advancements(&mut self) {
        use pumpkin_data::item::Item;
        use pumpkin_util::text::TextComponent;
        use pumpkin_world::item::ItemStack;

        // Story root
        let story_root = AdvancementData {
            id: ResourceLocation::from("minecraft:story/root"),
            parent: None,
            display: Some(AdvancementDisplayData {
                title: TextComponent::text("Minecraft"),
                description: TextComponent::text("The heart and story of the game"),
                icon: ItemStack::new(1, Item::from_id(27).unwrap_or(&Item::AIR)), // grass_block
                frame: AdvancementFrame::Task,
                background: Some(ResourceLocation::from(
                    "minecraft:gui/advancements/backgrounds/stone",
                )),
                show_toast: false,
                announce_to_chat: false,
                hidden: false,
                x: 0.0,
                y: 0.0,
            }),
            criteria: HashMap::from([(
                "crafting_table".to_string(),
                CriterionData {
                    trigger: ResourceLocation::from("minecraft:inventory_changed"),
                    conditions: serde_json::json!({
                        "items": [{"items": "minecraft:crafting_table"}]
                    }),
                },
            )]),
            requirements: vec![vec!["crafting_table".to_string()]],
            sends_telemetry_event: true,
        };

        // Stone Age
        let mine_stone = AdvancementData {
            id: ResourceLocation::from("minecraft:story/mine_stone"),
            parent: Some(ResourceLocation::from("minecraft:story/root")),
            display: Some(AdvancementDisplayData {
                title: TextComponent::text("Stone Age"),
                description: TextComponent::text("Mine Stone with your new Pickaxe"),
                icon: ItemStack::new(1, Item::from_id(35).unwrap_or(&Item::AIR)), // cobblestone
                frame: AdvancementFrame::Task,
                background: None,
                show_toast: true,
                announce_to_chat: true,
                hidden: false,
                x: 2.0,
                y: 0.0,
            }),
            criteria: HashMap::from([(
                "get_cobblestone".to_string(),
                CriterionData {
                    trigger: ResourceLocation::from("minecraft:inventory_changed"),
                    conditions: serde_json::json!({
                        "items": [{"items": "minecraft:cobblestone"}]
                    }),
                },
            )]),
            requirements: vec![vec!["get_cobblestone".to_string()]],
            sends_telemetry_event: true,
        };

        // Getting an Upgrade
        let upgrade_tools = AdvancementData {
            id: ResourceLocation::from("minecraft:story/upgrade_tools"),
            parent: Some(ResourceLocation::from("minecraft:story/mine_stone")),
            display: Some(AdvancementDisplayData {
                title: TextComponent::text("Getting an Upgrade"),
                description: TextComponent::text("Construct a better Pickaxe"),
                icon: ItemStack::new(1, Item::from_id(923).unwrap_or(&Item::AIR)), // stone_pickaxe
                frame: AdvancementFrame::Task,
                background: None,
                show_toast: true,
                announce_to_chat: true,
                hidden: false,
                x: 4.0,
                y: 0.0,
            }),
            criteria: HashMap::from([(
                "has_stone_pickaxe".to_string(),
                CriterionData {
                    trigger: ResourceLocation::from("minecraft:inventory_changed"),
                    conditions: serde_json::json!({
                        "items": [{"items": "minecraft:stone_pickaxe"}]
                    }),
                },
            )]),
            requirements: vec![vec!["has_stone_pickaxe".to_string()]],
            sends_telemetry_event: true,
        };

        // Acquire Hardware
        let smelt_iron = AdvancementData {
            id: ResourceLocation::from("minecraft:story/smelt_iron"),
            parent: Some(ResourceLocation::from("minecraft:story/upgrade_tools")),
            display: Some(AdvancementDisplayData {
                title: TextComponent::text("Acquire Hardware"),
                description: TextComponent::text("Smelt an Iron Ingot"),
                icon: ItemStack::new(1, Item::from_id(904).unwrap_or(&Item::AIR)), // iron_ingot
                frame: AdvancementFrame::Task,
                background: None,
                show_toast: true,
                announce_to_chat: true,
                hidden: false,
                x: 6.0,
                y: 0.0,
            }),
            criteria: HashMap::from([(
                "has_iron_ingot".to_string(),
                CriterionData {
                    trigger: ResourceLocation::from("minecraft:inventory_changed"),
                    conditions: serde_json::json!({
                        "items": [{"items": "minecraft:iron_ingot"}]
                    }),
                },
            )]),
            requirements: vec![vec!["has_iron_ingot".to_string()]],
            sends_telemetry_event: true,
        };

        // Add to registry
        self.roots.push(story_root.id.clone());
        self.advancements
            .insert(story_root.id.clone(), Arc::new(story_root));
        self.advancements
            .insert(mine_stone.id.clone(), Arc::new(mine_stone));
        self.advancements
            .insert(upgrade_tools.id.clone(), Arc::new(upgrade_tools));
        self.advancements
            .insert(smelt_iron.id.clone(), Arc::new(smelt_iron));
    }
}

impl Default for AdvancementRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Thread-safe wrapper for the advancement registry.
pub type SharedAdvancementRegistry = Arc<RwLock<AdvancementRegistry>>;
