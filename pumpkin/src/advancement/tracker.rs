//! Player advancement tracker - tracks progress per player.

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;

use std::sync::RwLock;
use pumpkin_protocol::codec::item_stack_seralizer::ItemStackSerializer;
use pumpkin_protocol::java::client::play::{
    Advancement, AdvancementDisplay, AdvancementFrameType, AdvancementMapping,
    AdvancementProgressMapping, CUpdateAdvancements,
};
use pumpkin_util::resource_location::ResourceLocation;
use pumpkin_util::text::TextComponent;
use pumpkin_world::item::ItemStack;

use super::{AdvancementData, AdvancementProgressData, AdvancementRegistry, flags, loader};
use crate::entity::player::Player;

/// Tracks advancement progress for a single player.
pub struct PlayerAdvancementTracker {
    /// Reference to the global advancement registry.
    registry: Arc<RwLock<AdvancementRegistry>>,
    /// Progress for each advancement.
    progress: HashMap<ResourceLocation, AdvancementProgressData>,
    /// Advancements visible to this player.
    visible: HashSet<ResourceLocation>,
    /// Advancements that need to be sent to client.
    pending_updates: HashSet<ResourceLocation>,
    /// Advancements to remove from client.
    pending_removals: HashSet<ResourceLocation>,
    /// Whether initial sync is needed.
    dirty: bool,
}

impl PlayerAdvancementTracker {
    /// Creates a new tracker for a player.
    #[must_use]
    pub fn new(registry: Arc<RwLock<AdvancementRegistry>>) -> Self {
        let mut tracker = Self {
            registry,
            progress: HashMap::new(),
            visible: HashSet::new(),
            pending_updates: HashSet::new(),
            pending_removals: HashSet::new(),
            dirty: true,
        };
        tracker.init_progress();
        tracker
    }

    /// Initializes progress for all advancements.
    fn init_progress(&mut self) {
        let registry = self.registry.read().unwrap();
        for advancement in registry.all() {
            let mut progress = AdvancementProgressData::new();
            progress.init(&advancement.requirements);
            self.progress.insert(advancement.id.clone(), progress);
        }
    }

    /// Grants a criterion for an advancement.
    /// Returns true if the criterion was newly granted.
    pub fn grant_criterion(&mut self, advancement_id: &ResourceLocation, criterion: &str) -> bool {
        if let Some(progress) = self.progress.get_mut(advancement_id)
            && progress.grant(criterion)
        {
            self.pending_updates.insert(advancement_id.clone());

            // Check if advancement is now complete
            let is_complete = {
                let registry = self.registry.read().unwrap();
                registry
                    .get(advancement_id)
                    .is_some_and(|advancement| {
                        progress.is_done(&advancement.requirements)
                    })
            };
            if is_complete {
                // Advancement completed!
                self.on_advancement_complete(advancement_id);
            }
            return true;
        }
        false
    }

    /// Called when an advancement is completed.
    #[allow(clippy::unused_self)]
    fn on_advancement_complete(&self, _advancement_id: &ResourceLocation) {
        // TODO: Apply rewards, announce to chat, etc.
    }

    /// Revokes a criterion for an advancement.
    pub fn revoke_criterion(&mut self, advancement_id: &ResourceLocation, criterion: &str) -> bool {
        if let Some(progress) = self.progress.get_mut(advancement_id)
            && progress.revoke(criterion)
        {
            self.pending_updates.insert(advancement_id.clone());
            return true;
        }
        false
    }

    /// Gets progress for an advancement.
    #[must_use]
    pub fn get_progress(&self, advancement_id: &ResourceLocation) -> Option<&AdvancementProgressData> {
        self.progress.get(advancement_id)
    }

    /// Checks if an advancement is complete.
    #[must_use]
    pub fn is_complete(&self, advancement_id: &ResourceLocation) -> bool {
        let registry = self.registry.read().unwrap();
        if let (Some(progress), Some(advancement)) = (
            self.progress.get(advancement_id),
            registry.get(advancement_id),
        ) {
            progress.is_done(&advancement.requirements)
        } else {
            false
        }
    }

    /// Sends advancement updates to the player.
    ///
    /// TODO: This method needs to properly convert owned structures to packet structures.
    /// For now, use `send_advancements()` which uses the simpler test implementation.
    #[allow(dead_code, clippy::unused_async)]
    pub async fn send_update(&mut self, _player: &Arc<Player>, _show_toast: bool) {
        if !self.dirty && self.pending_updates.is_empty() && self.pending_removals.is_empty() {
            return;
        }

        // TODO: Implement proper packet building with lifetime management
        // The challenge is converting owned AdvancementMappingOwned to borrowed AdvancementMapping
        // This requires careful lifetime management or a different packet structure

        // Clear pending state
        self.dirty = false;
        self.pending_updates.clear();
        self.pending_removals.clear();
    }

    /// Calculates which advancements should be visible.
    #[allow(dead_code)]
    fn calculate_visibility(&mut self, registry: &AdvancementRegistry) {
        self.visible.clear();

        // An advancement is visible if:
        // 1. It has no parent (root) and has display, OR
        // 2. Its parent is complete and it has display
        for advancement in registry.all() {
            if advancement.display.is_none() {
                continue;
            }

            let should_show = advancement
                .parent
                .as_ref()
                .is_none_or(|parent_id| self.is_complete(parent_id));

            if should_show {
                self.visible.insert(advancement.id.clone());
            }
        }
    }

    /// Builds advancement mappings for the packet.
    #[allow(dead_code)]
    fn build_advancement_mappings<'a>(
        &'a self,
        advancements: &[(&'a AdvancementData, &'a AdvancementProgressData)],
    ) -> Vec<AdvancementMappingOwned> {
        advancements
            .iter()
            .map(|(adv, _)| self.advancement_to_mapping(adv))
            .collect()
    }

    /// Converts an advancement to a packet mapping.
    #[allow(dead_code, clippy::unused_self)]
    fn advancement_to_mapping(&self, adv: &AdvancementData) -> AdvancementMappingOwned {
        let display = adv.display.as_ref().map(|d| AdvancementDisplayOwned {
            title: d.title.clone(),
            description: d.description.clone(),
            icon: d.icon.clone(),
            frame_type: match d.frame {
                super::AdvancementFrame::Task => AdvancementFrameType::Task,
                super::AdvancementFrame::Challenge => AdvancementFrameType::Challenge,
                super::AdvancementFrame::Goal => AdvancementFrameType::Goal,
            },
            flags: {
                let mut f = 0;
                if d.background.is_some() {
                    f |= flags::HAS_BACKGROUND;
                }
                if d.show_toast {
                    f |= flags::SHOW_TOAST;
                }
                if d.hidden {
                    f |= flags::HIDDEN;
                }
                f
            },
            background_texture: d.background.clone(),
            x: d.x,
            y: d.y,
        });

        let requirements: Vec<Vec<String>> = adv.requirements.clone();

        AdvancementMappingOwned {
            id: adv.id.clone(),
            parent: adv.parent.clone(),
            display,
            requirements,
            sends_telemetry_event: adv.sends_telemetry_event,
        }
    }

    /// Builds progress mappings for the packet.
    #[allow(dead_code, clippy::unused_self)]
    fn build_progress_mappings<'a>(
        &'a self,
        progress: &[(&'a ResourceLocation, &'a AdvancementProgressData)],
    ) -> Vec<AdvancementProgressMappingOwned> {
        progress
            .iter()
            .map(|(id, prog)| AdvancementProgressMappingOwned {
                id: (*id).clone(),
                criteria: prog
                    .criteria
                    .iter()
                    .filter_map(|(name, time)| {
                        time.map(|t| (name.clone(), t))
                    })
                    .collect(),
            })
            .collect()
    }
}

// Owned versions of packet structures for easier lifetime management
// TODO: These will be used when implementing the full tracker

#[allow(dead_code)]
struct AdvancementMappingOwned {
    id: ResourceLocation,
    parent: Option<ResourceLocation>,
    display: Option<AdvancementDisplayOwned>,
    requirements: Vec<Vec<String>>,
    sends_telemetry_event: bool,
}

#[allow(dead_code)]
struct AdvancementDisplayOwned {
    title: pumpkin_util::text::TextComponent,
    description: pumpkin_util::text::TextComponent,
    icon: pumpkin_world::item::ItemStack,
    frame_type: AdvancementFrameType,
    flags: i32,
    background_texture: Option<ResourceLocation>,
    x: f32,
    y: f32,
}

#[allow(dead_code)]
struct AdvancementProgressMappingOwned {
    id: ResourceLocation,
    criteria: Vec<(String, i64)>,
}

/// Sends initial advancements to a player.
/// This is the main entry point called when a player joins.
pub async fn send_advancements(player: &Arc<Player>) {
    send_loaded_advancements(player).await;
}

/// Prepared data for a single advancement (owned, for lifetime management).
struct PreparedAdvancement {
    id: ResourceLocation,
    parent: Option<ResourceLocation>,
    title: TextComponent,
    description: TextComponent,
    icon: ItemStack,
    frame: super::AdvancementFrame,
    background: Option<ResourceLocation>,
    show_toast: bool,
    hidden: bool,
    x: f32,
    y: f32,
    requirements: Vec<Vec<String>>,
    sends_telemetry_event: bool,
    has_display: bool,
}

/// Sends advancements loaded from JSON files.
#[expect(clippy::too_many_lines)]
async fn send_loaded_advancements(player: &Arc<Player>) {
    // Load advancements from data directory
    let data_path = Path::new("pumpkin/src/data/minecraft");
    let advancements_data = match loader::load_advancements_from_dir(data_path, "minecraft") {
        Ok(data) => {
            log::info!("Loaded {} advancements from JSON files", data.len());
            data
        }
        Err(e) => {
            log::warn!("Failed to load advancements: {e}, using empty set");
            Vec::new()
        }
    };

    if advancements_data.is_empty() {
        log::warn!("No advancements loaded, skipping send");
        return;
    }

    // Prepare owned data for all advancements
    let prepared: Vec<PreparedAdvancement> = advancements_data
        .iter()
        .map(|adv| {
            let (title, description, icon, frame, background, show_toast, hidden, x, y, has_display) =
                adv.display.as_ref().map_or_else(
                    || {
                        // Hidden advancement (no display) - use defaults
                        (
                            TextComponent::text(""),
                            TextComponent::text(""),
                            ItemStack::new(1, &pumpkin_data::item::Item::AIR),
                            super::AdvancementFrame::Task,
                            None,
                            false,
                            true,
                            0.0,
                            0.0,
                            false,
                        )
                    },
                    |d| {
                        (
                            d.title.clone(),
                            d.description.clone(),
                            d.icon.clone(),
                            d.frame,
                            d.background.clone(),
                            d.show_toast,
                            d.hidden,
                            d.x,
                            d.y,
                            true,
                        )
                    },
                );

            PreparedAdvancement {
                id: adv.id.clone(),
                parent: adv.parent.clone(),
                title,
                description,
                icon,
                frame,
                background,
                show_toast,
                hidden,
                x,
                y,
                requirements: adv.requirements.clone(),
                sends_telemetry_event: adv.sends_telemetry_event,
                has_display,
            }
        })
        .collect();

    // Build icon serializers (need to live as long as displays)
    let icons: Vec<ItemStackSerializer> = prepared
        .iter()
        .map(|p| ItemStackSerializer(Cow::Owned(p.icon.clone())))
        .collect();

    // Build requirements as &[&[&str]]
    let requirements_owned: Vec<Vec<Vec<&str>>> = prepared
        .iter()
        .map(|p| {
            p.requirements
                .iter()
                .map(|group| group.iter().map(String::as_str).collect())
                .collect()
        })
        .collect();

    let requirements_refs: Vec<Vec<&[&str]>> = requirements_owned
        .iter()
        .map(|groups| groups.iter().map(Vec::as_slice).collect())
        .collect();

    // Build advancement mappings with displays inline
    let advancements: Vec<AdvancementMapping> = prepared
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let display = p.has_display.then(|| {
                let mut f = 0;
                if p.background.is_some() {
                    f |= flags::HAS_BACKGROUND;
                }
                if p.show_toast {
                    f |= flags::SHOW_TOAST;
                }
                if p.hidden {
                    f |= flags::HIDDEN;
                }

                AdvancementDisplay {
                    title: &p.title,
                    description: &p.description,
                    icon: &icons[i],
                    frame_type: match p.frame {
                        super::AdvancementFrame::Task => AdvancementFrameType::Task,
                        super::AdvancementFrame::Challenge => AdvancementFrameType::Challenge,
                        super::AdvancementFrame::Goal => AdvancementFrameType::Goal,
                    },
                    flags: f,
                    background_texture: p.background.clone(),
                    x: p.x,
                    y: p.y,
                }
            });

            AdvancementMapping {
                id: p.id.clone(),
                advancement: Advancement {
                    parent: p.parent.clone(),
                    display,
                    requirements: &requirements_refs[i],
                    sends_telemetry_event: p.sends_telemetry_event,
                },
            }
        })
        .collect();

    // No progress to send initially (player hasn't earned any yet)
    let progress: Vec<AdvancementProgressMapping> = Vec::new();

    let packet = CUpdateAdvancements::new(true, &advancements, &[], &progress, false);
    player.client.enqueue_packet(&packet).await;
}
