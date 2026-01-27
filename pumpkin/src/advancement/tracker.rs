//! Player advancement tracker - tracks progress per player.

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;

use std::sync::RwLock;
use pumpkin_protocol::codec::item_stack_seralizer::ItemStackSerializer;
use pumpkin_protocol::java::client::play::{
    Advancement, AdvancementDisplay, AdvancementFrameType, AdvancementMapping,
    AdvancementProgress, AdvancementProgressMapping, CUpdateAdvancements, CriterionProgress,
    CriterionProgressMapping,
};
use pumpkin_util::resource_location::ResourceLocation;
use pumpkin_util::text::TextComponent;
use pumpkin_world::item::ItemStack;
use uuid::Uuid;

use super::{storage, AdvancementData, AdvancementProgressData, AdvancementRegistry, flags};
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

    /// Loads progress from disk and merges with current progress.
    ///
    /// This should be called after creating the tracker to restore saved progress.
    pub fn load_progress(&mut self, world_path: &Path, player_uuid: Uuid) {
        let loaded = storage::load_progress(world_path, player_uuid);

        // Merge loaded progress with initialized progress
        for (id, loaded_progress) in loaded {
            if let Some(current) = self.progress.get_mut(&id) {
                // Merge criteria - keep structure but restore obtained times
                for (criterion, time) in loaded_progress.criteria {
                    if let Some(current_time) = current.criteria.get_mut(&criterion) {
                        *current_time = time;
                    }
                }
            }
        }

        log::debug!("Loaded advancement progress for {player_uuid}");
    }

    /// Saves progress to disk.
    ///
    /// This should be called periodically or when the player disconnects.
    pub fn save_progress(&self, world_path: &Path, player_uuid: Uuid) {
        // Build requirements map for checking completion
        let requirements: HashMap<ResourceLocation, Vec<Vec<String>>> = {
            let registry = self.registry.read().unwrap();
            registry
                .all()
                .map(|adv| (adv.id.clone(), adv.requirements.clone()))
                .collect()
        };

        if let Err(e) = storage::save_progress(world_path, player_uuid, &self.progress, &requirements)
        {
            log::error!("Failed to save advancement progress for {player_uuid}: {e}");
        }
    }

    /// Checks if there are any pending changes that should be saved.
    #[must_use]
    pub fn has_unsaved_changes(&self) -> bool {
        !self.pending_updates.is_empty()
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
///
/// This function also loads the player's saved progress from disk and
/// checks their current inventory for any advancement triggers.
pub async fn send_advancements(player: &Arc<Player>) {
    // Load saved progress from disk
    let world_path = player
        .living_entity
        .entity
        .world
        .level
        .level_folder
        .root_folder
        .clone();
    let player_uuid = player.gameprofile.id;

    {
        let mut tracker = player.advancement_tracker.write().await;
        tracker.load_progress(&world_path, player_uuid);
    }

    // Check current inventory for any advancement triggers
    // This handles items the player had before disconnecting
    super::trigger::check_inventory_for_advancements(player).await;

    send_loaded_advancements(player).await;
}

/// Saves the player's advancement progress to disk.
///
/// This should be called when the player disconnects or periodically.
pub async fn save_advancements(player: &Arc<Player>) {
    let world_path = player
        .living_entity
        .entity
        .world
        .level
        .level_folder
        .root_folder
        .clone();
    let player_uuid = player.gameprofile.id;

    let tracker = player.advancement_tracker.read().await;
    tracker.save_progress(&world_path, player_uuid);
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

/// Sends advancements to player using their tracker.
#[expect(clippy::too_many_lines)]
async fn send_loaded_advancements(player: &Arc<Player>) {
    // Collect all data from tracker before any awaits to avoid Send issues
    let (advancements_data, progress_snapshot) = {
        let tracker = player.advancement_tracker.read().await;
        let registry = tracker.registry.read().unwrap();
        let advancements: Vec<_> = registry.all().cloned().collect();
        drop(registry);

        // Snapshot progress for all advancements
        let progress: HashMap<ResourceLocation, super::AdvancementProgressData> = advancements
            .iter()
            .filter_map(|adv| {
                tracker.get_progress(&adv.id).cloned().map(|p| (adv.id.clone(), p))
            })
            .collect();

        (advancements, progress)
    };

    if advancements_data.is_empty() {
        log::warn!("No advancements in registry, skipping send");
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

    // Build progress mappings from progress_snapshot (collected earlier)
    // Filter to only advancements that have any obtained criteria
    let progress_data: Vec<(ResourceLocation, super::AdvancementProgressData)> = prepared
        .iter()
        .filter_map(|p| {
            let prog = progress_snapshot.get(&p.id)?;
            if prog.is_any_obtained() {
                Some((p.id.clone(), prog.clone()))
            } else {
                None
            }
        })
        .collect();

    // Build criterion progress for each advancement with progress
    let criterion_progress: Vec<Vec<CriterionProgressMapping>> = progress_data
        .iter()
        .map(|(_, prog): &(ResourceLocation, super::AdvancementProgressData)| {
            prog.criteria
                .iter()
                .filter_map(|(name, time): (&String, &Option<i64>)| {
                    time.map(|t| CriterionProgressMapping {
                        criterion: name.as_str(),
                        progress: CriterionProgress {
                            obtained_time: Some(t),
                        },
                    })
                })
                .collect()
        })
        .collect();

    // Build progress mappings - create AdvancementProgress inline to avoid clone
    let progress: Vec<AdvancementProgressMapping> = progress_data
        .iter()
        .enumerate()
        .map(|(i, (id, _)): (usize, &(ResourceLocation, super::AdvancementProgressData))| {
            AdvancementProgressMapping {
                id: id.clone(),
                progress: AdvancementProgress {
                    criteria: &criterion_progress[i],
                },
            }
        })
        .collect();

    let packet = CUpdateAdvancements::new(true, &advancements, &[], &progress, false);
    player.client.enqueue_packet(&packet).await;
}
