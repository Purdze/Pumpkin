//! Advancement system for Pumpkin.
//!
//! This module implements the vanilla-compatible advancement system using the
//! listener pattern for O(1) trigger performance.
//!
//! ## Architecture
//!
//! - `AdvancementRegistry` - Holds all loaded advancements (from datapacks)
//! - `PlayerAdvancementTracker` - Per-player progress tracking
//! - `Criterion` trait - Trigger types (`inventory_changed`, `changed_dimension`, etc.)
//! - `AbstractCriterion<T>` - Base implementation with player -> conditions map

pub mod criterion;
pub mod loader;
pub mod registry;
pub mod storage;
pub mod tracker;
pub mod trigger;

use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use pumpkin_util::resource_location::ResourceLocation;
use pumpkin_util::text::TextComponent;
use pumpkin_world::item::ItemStack;
use serde::Deserialize;

pub use registry::AdvancementRegistry;
pub use tracker::{PlayerAdvancementTracker, save_advancements, send_advancements};

/// Gets the current timestamp in milliseconds since Unix epoch.
#[must_use]
pub fn current_timestamp_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Display flags for advancements.
pub mod flags {
    /// Advancement has a background texture.
    pub const HAS_BACKGROUND: i32 = 0x1;
    /// Show toast notification when completed.
    pub const SHOW_TOAST: i32 = 0x2;
    /// Hidden until completed or has completed child.
    pub const HIDDEN: i32 = 0x4;
}

/// Frame type for advancement display.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdvancementFrame {
    #[default]
    Task,
    Challenge,
    Goal,
}

/// An advancement definition loaded from JSON.
#[derive(Clone)]
pub struct AdvancementData {
    /// Unique identifier for this advancement.
    pub id: ResourceLocation,
    /// Parent advancement ID (None for root advancements).
    pub parent: Option<ResourceLocation>,
    /// Display information (None for hidden/recipe advancements).
    pub display: Option<AdvancementDisplayData>,
    /// Criteria that must be completed (name -> trigger type).
    pub criteria: HashMap<String, CriterionData>,
    /// Requirements - groups of criteria (AND of ORs).
    pub requirements: Vec<Vec<String>>,
    /// Whether to send telemetry when completed.
    pub sends_telemetry_event: bool,
}

/// Display information for an advancement.
#[derive(Clone)]
pub struct AdvancementDisplayData {
    /// Title text.
    pub title: TextComponent,
    /// Description text.
    pub description: TextComponent,
    /// Icon item.
    pub icon: ItemStack,
    /// Frame type (task, challenge, goal).
    pub frame: AdvancementFrame,
    /// Background texture (only for root advancements).
    pub background: Option<ResourceLocation>,
    /// Whether to show toast notification.
    pub show_toast: bool,
    /// Whether to announce in chat.
    pub announce_to_chat: bool,
    /// Whether hidden until completed.
    pub hidden: bool,
    /// X position in the advancement tree.
    pub x: f32,
    /// Y position in the advancement tree.
    pub y: f32,
}

/// Criterion definition from advancement JSON.
#[derive(Debug, Clone)]
pub struct CriterionData {
    /// The trigger type (e.g., `minecraft:inventory_changed`).
    pub trigger: ResourceLocation,
    /// Conditions for this trigger (trigger-specific).
    pub conditions: serde_json::Value,
}

/// Progress for a single advancement.
#[derive(Debug, Clone, Default)]
pub struct AdvancementProgressData {
    /// Progress for each criterion (name -> obtained time).
    pub criteria: HashMap<String, Option<i64>>,
}

impl AdvancementProgressData {
    /// Creates new empty progress.
    #[must_use]
    pub fn new() -> Self {
        Self {
            criteria: HashMap::new(),
        }
    }

    /// Initializes progress for the given requirements.
    pub fn init(&mut self, requirements: &[Vec<String>]) {
        for group in requirements {
            for criterion in group {
                self.criteria.entry(criterion.clone()).or_insert(None);
            }
        }
    }

    /// Grants a criterion, returning true if it wasn't already granted.
    pub fn grant(&mut self, criterion: &str) -> bool {
        if let Some(time) = self.criteria.get_mut(criterion)
            && time.is_none()
        {
            *time = Some(current_timestamp_millis());
            return true;
        }
        false
    }

    /// Revokes a criterion, returning true if it was granted.
    pub fn revoke(&mut self, criterion: &str) -> bool {
        if let Some(time) = self.criteria.get_mut(criterion)
            && time.is_some()
        {
            *time = None;
            return true;
        }
        false
    }

    /// Checks if a criterion is obtained.
    #[must_use]
    pub fn is_criterion_obtained(&self, criterion: &str) -> bool {
        self.criteria.get(criterion).is_some_and(Option::is_some)
    }

    /// Checks if this advancement is complete based on requirements.
    #[must_use]
    pub fn is_done(&self, requirements: &[Vec<String>]) -> bool {
        // All requirement groups must have at least one criterion obtained
        requirements.iter().all(|group| {
            group.iter().any(|criterion| self.is_criterion_obtained(criterion))
        })
    }

    /// Checks if any criterion has been obtained.
    #[must_use]
    pub fn is_any_obtained(&self) -> bool {
        self.criteria.values().any(Option::is_some)
    }
}
