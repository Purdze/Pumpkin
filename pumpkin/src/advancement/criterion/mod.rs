//! Criterion system for advancements.
//!
//! Each criterion type maintains a map of `player -> Set<conditions>` for O(1) trigger lookups.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use pumpkin_util::resource_location::ResourceLocation;

/// A criterion that can be triggered.
pub trait Criterion: Send + Sync {
    /// The type of conditions this criterion checks.
    type Conditions: CriterionConditions;

    /// Returns the trigger ID for this criterion.
    fn trigger_id(&self) -> &ResourceLocation;
}

/// Conditions that must be met for a criterion to be granted.
pub trait CriterionConditions: Send + Sync + Clone {
    /// Gets the advancement ID this condition is for.
    fn advancement_id(&self) -> &ResourceLocation;

    /// Gets the criterion name within the advancement.
    fn criterion_name(&self) -> &str;
}

/// Container for criterion conditions with metadata.
#[derive(Clone)]
pub struct ConditionsContainer<T: CriterionConditions> {
    /// The advancement this criterion belongs to.
    pub advancement_id: ResourceLocation,
    /// The criterion name within the advancement.
    pub criterion_name: String,
    /// The actual conditions to check.
    pub conditions: T,
}

/// A generic criterion implementation with player tracking.
///
/// This is the key pattern from vanilla - instead of checking all advancements
/// on every trigger, we track which conditions each player needs checked.
pub struct AbstractCriterion<T: CriterionConditions> {
    /// Trigger ID for this criterion type.
    trigger_id: ResourceLocation,
    /// Map: `player_id` -> Set<conditions tracked for that player>
    /// This is what makes triggers O(1) instead of O(all advancements)
    progressions: std::sync::RwLock<HashMap<u128, HashSet<Arc<ConditionsContainer<T>>>>>,
}

impl<T: CriterionConditions + 'static> AbstractCriterion<T> {
    /// Creates a new abstract criterion with the given trigger ID.
    #[must_use]
    pub fn new(trigger_id: ResourceLocation) -> Self {
        Self {
            trigger_id,
            progressions: std::sync::RwLock::new(HashMap::new()),
        }
    }

    /// Starts tracking conditions for a player.
    pub fn start_tracking(&self, player_id: u128, container: Arc<ConditionsContainer<T>>) {
        let mut progressions = self.progressions.write().unwrap();
        progressions.entry(player_id).or_default().insert(container);
    }

    /// Stops tracking conditions for a player.
    pub fn stop_tracking(&self, player_id: u128, advancement_id: &ResourceLocation) {
        let mut progressions = self.progressions.write().unwrap();
        if let Some(conditions) = progressions.get_mut(&player_id) {
            conditions.retain(|c| &c.advancement_id != advancement_id);
        }
    }

    /// Removes all tracking for a player (called on disconnect).
    pub fn remove_player(&self, player_id: u128) {
        let mut progressions = self.progressions.write().unwrap();
        progressions.remove(&player_id);
    }
}

impl<T: CriterionConditions + 'static> Criterion for AbstractCriterion<T> {
    type Conditions = T;

    fn trigger_id(&self) -> &ResourceLocation {
        &self.trigger_id
    }
}

// Implement Hash and Eq for ConditionsContainer based on advancement and criterion name
impl<T: CriterionConditions> std::hash::Hash for ConditionsContainer<T> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.advancement_id.hash(state);
        self.criterion_name.hash(state);
    }
}

impl<T: CriterionConditions> PartialEq for ConditionsContainer<T> {
    fn eq(&self, other: &Self) -> bool {
        self.advancement_id == other.advancement_id && self.criterion_name == other.criterion_name
    }
}

impl<T: CriterionConditions> Eq for ConditionsContainer<T> {}
