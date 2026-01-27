//! Loads advancements from JSON files (datapack format).

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use include_dir::{include_dir, Dir};
use pumpkin_data::item::Item;
use pumpkin_util::resource_location::ResourceLocation;
use pumpkin_util::text::TextComponent;
use pumpkin_world::item::ItemStack;
use serde::Deserialize;

use super::{AdvancementData, AdvancementDisplayData, AdvancementFrame, CriterionData};

/// Embedded advancement data from compile time.
static EMBEDDED_ADVANCEMENTS: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/src/data/minecraft/advancement");

/// Loads all advancements from embedded data (compiled into the binary).
/// This is the primary method - advancements are embedded like vanilla Minecraft.
pub fn load_embedded_advancements(namespace: &str) -> Vec<AdvancementData> {
    let mut advancements = Vec::new();
    load_embedded_recursive(&EMBEDDED_ADVANCEMENTS, namespace, "", &mut advancements);

    // Calculate layout positions based on tree structure
    calculate_layout(&mut advancements);

    log::info!("Loaded {} embedded advancements", advancements.len());
    advancements
}

/// Recursively load advancements from an embedded directory.
fn load_embedded_recursive(
    dir: &Dir<'_>,
    namespace: &str,
    prefix: &str,
    advancements: &mut Vec<AdvancementData>,
) {
    // Process files in this directory
    for file in dir.files() {
        if let Some(ext) = file.path().extension() {
            if ext == "json" {
                let file_name = file.path().file_stem().unwrap().to_string_lossy();
                let advancement_id = if prefix.is_empty() {
                    format!("{namespace}:{file_name}")
                } else {
                    format!("{namespace}:{prefix}/{file_name}")
                };

                if let Some(content) = file.contents_utf8() {
                    match serde_json::from_str::<JsonAdvancement>(content) {
                        Ok(json) => advancements.push(convert_json_advancement(json, &advancement_id)),
                        Err(e) => log::warn!("Failed to parse advancement {advancement_id}: {e}"),
                    }
                }
            }
        }
    }

    // Recurse into subdirectories
    for subdir in dir.dirs() {
        let dir_name = subdir.path().file_name().unwrap().to_string_lossy();
        let new_prefix = if prefix.is_empty() {
            dir_name.to_string()
        } else {
            format!("{prefix}/{dir_name}")
        };
        load_embedded_recursive(subdir, namespace, &new_prefix, advancements);
    }
}

/// Loads all advancements from a directory (recursively) and calculates layout positions.
/// Expected structure: `<base_path>/advancement/**/*.json`
pub fn load_advancements_from_dir(
    base_path: &Path,
    namespace: &str,
) -> Result<Vec<AdvancementData>, LoadError> {
    let mut advancements = Vec::new();

    // Support both `data/advancement/` and `data/minecraft/advancement/` structures
    let advancement_path = if base_path.join("advancement").exists() {
        base_path.join("advancement")
    } else {
        base_path.join(namespace).join("advancement")
    };

    if !advancement_path.exists() {
        return Err(LoadError::PathNotFound(advancement_path.display().to_string()));
    }

    load_advancements_recursive(&advancement_path, namespace, "", &mut advancements)?;

    // Calculate layout positions based on tree structure
    calculate_layout(&mut advancements);

    Ok(advancements)
}

/// Calculates x/y positions for advancements using the Buchheim algorithm.
/// This matches vanilla Minecraft's AdvancementPositioner exactly.
fn calculate_layout(advancements: &mut [AdvancementData]) {
    // Build children map
    let mut children_map: HashMap<ResourceLocation, Vec<usize>> = HashMap::new();
    let mut roots: Vec<usize> = Vec::new();

    for (idx, adv) in advancements.iter().enumerate() {
        if let Some(parent_id) = &adv.parent {
            children_map.entry(parent_id.clone()).or_default().push(idx);
        } else if adv.display.is_some() {
            roots.push(idx);
        }
    }

    // Sort children to match vanilla's effective ordering
    for children in children_map.values_mut() {
        children.sort_by(|a, b| {
            let path_a = advancements[*a].id.path.as_str();
            let path_b = advancements[*b].id.path.as_str();

            // Get the last segment (after last /)
            let name_a = path_a.rsplit('/').next().unwrap_or(path_a);
            let name_b = path_b.rsplit('/').next().unwrap_or(path_b);

            // Special cases to match vanilla's ordering
            let suffix_priority = |name: &str| -> i32 {
                if name.ends_with("_gear") {
                    0
                } else if name.ends_with("_item") {
                    1
                } else if name.ends_with("_villager") {
                    2
                } else if name.ends_with("_eye") {
                    3
                } else {
                    100 // Default - use other sorting
                }
            };

            let priority_a = suffix_priority(name_a);
            let priority_b = suffix_priority(name_b);

            if priority_a < 100 || priority_b < 100 {
                // One has a special suffix, use priority
                return priority_a.cmp(&priority_b).then_with(|| path_a.cmp(path_b));
            }

            // Default: sort by last character ascending, then full path
            let last_a = name_a.chars().last().unwrap_or('\0');
            let last_b = name_b.chars().last().unwrap_or('\0');
            last_a.cmp(&last_b).then_with(|| path_a.cmp(path_b))
        });
    }

    // Process each tree independently
    for root_idx in roots {
        let positioners =
            AdvancementPositioner::arrange_for_tree(advancements, &children_map, root_idx);

        // Apply positions to advancements
        for pos in &positioners {
            if let Some(display) = &mut advancements[pos.advancement_idx].display {
                display.x = pos.depth as f32;
                display.y = pos.row;
            }
        }
    }
}

/// Buchheim-Jünger-Leipert tree layout algorithm.
/// This is a direct port of vanilla's AdvancementPositioner.java
struct AdvancementPositioner {
    /// Index into the advancements array
    advancement_idx: usize,
    /// Index of parent positioner in the positioners vec (None for root)
    parent_idx: Option<usize>,
    /// Index of previous sibling positioner
    previous_sibling_idx: Option<usize>,
    /// Position among siblings (1-indexed like vanilla)
    children_size: i32,
    /// Child positioner indices
    children: Vec<usize>,
    /// For thread traversal
    optional_last_idx: usize,
    /// Substitute child for thread
    substitute_child_idx: Option<usize>,
    /// X position (depth in tree)
    depth: i32,
    /// Y position (row)
    row: f32,
    /// Relative row offset for siblings
    relative_row_in_siblings: f32,
    /// Shift values for collision resolution
    shift_acceleration: f32,
    shift_change: f32,
}

impl AdvancementPositioner {
    /// Main entry point - arranges the entire tree
    fn arrange_for_tree(
        advancements: &[AdvancementData],
        children_map: &HashMap<ResourceLocation, Vec<usize>>,
        root_adv_idx: usize,
    ) -> Vec<AdvancementPositioner> {
        let mut positioners: Vec<AdvancementPositioner> = Vec::new();

        // Build the positioner tree recursively
        Self::build_tree_recursive(
            advancements,
            children_map,
            &mut positioners,
            root_adv_idx,
            None,
            None,
            1,
            0,
        );

        if positioners.is_empty() {
            return positioners;
        }

        // Phase 1: Calculate initial positions
        Self::calculate_recursive(&mut positioners, 0);

        // Phase 2: Find minimum row and shift if needed
        let min_row = Self::find_min_row_recursive(&mut positioners, 0, 0.0, 0);
        if min_row < 0.0 {
            Self::increase_row_recursive(&mut positioners, 0, -min_row);
        }

        positioners
    }

    /// Builds the positioner tree recursively
    fn build_tree_recursive(
        advancements: &[AdvancementData],
        children_map: &HashMap<ResourceLocation, Vec<usize>>,
        positioners: &mut Vec<AdvancementPositioner>,
        adv_idx: usize,
        parent_pos_idx: Option<usize>,
        previous_sibling_pos_idx: Option<usize>,
        children_size: i32,
        depth: i32,
    ) -> usize {
        let my_idx = positioners.len();

        let pos = AdvancementPositioner {
            advancement_idx: adv_idx,
            parent_idx: parent_pos_idx,
            previous_sibling_idx: previous_sibling_pos_idx,
            children_size,
            children: Vec::new(),
            optional_last_idx: my_idx,
            substitute_child_idx: None,
            depth,
            row: -1.0,
            relative_row_in_siblings: 0.0,
            shift_acceleration: 0.0,
            shift_change: 0.0,
        };

        positioners.push(pos);

        // Get children for this advancement
        let id = &advancements[adv_idx].id;
        if let Some(child_adv_indices) = children_map.get(id) {
            let mut last_child_pos_idx: Option<usize> = None;
            let mut child_num = 0;

            for &child_adv_idx in child_adv_indices {
                // Only include children with display
                if advancements[child_adv_idx].display.is_some() {
                    child_num += 1;
                    let child_pos_idx = Self::build_tree_recursive(
                        advancements,
                        children_map,
                        positioners,
                        child_adv_idx,
                        Some(my_idx),
                        last_child_pos_idx,
                        child_num,
                        depth + 1,
                    );

                    positioners[my_idx].children.push(child_pos_idx);
                    last_child_pos_idx = Some(child_pos_idx);
                }
            }
        }

        my_idx
    }

    /// First pass: calculate row positions recursively
    fn calculate_recursive(positioners: &mut Vec<AdvancementPositioner>, idx: usize) {
        let children = positioners[idx].children.clone();

        if children.is_empty() {
            // Leaf node
            if let Some(prev_idx) = positioners[idx].previous_sibling_idx {
                positioners[idx].row = positioners[prev_idx].row + 1.0;
            } else {
                positioners[idx].row = 0.0;
            }
        } else {
            // Internal node - process children first
            let mut default_ancestor_idx = children[0];

            for &child_idx in &children {
                Self::calculate_recursive(positioners, child_idx);
                default_ancestor_idx =
                    Self::on_finish_calculation(positioners, child_idx, default_ancestor_idx);
            }

            Self::on_finish_children_calculation(positioners, idx);

            // Center on children
            let first_child_row = positioners[children[0]].row;
            let last_child_row = positioners[*children.last().unwrap()].row;
            let mid = (first_child_row + last_child_row) / 2.0;

            if let Some(prev_idx) = positioners[idx].previous_sibling_idx {
                positioners[idx].row = positioners[prev_idx].row + 1.0;
                positioners[idx].relative_row_in_siblings = positioners[idx].row - mid;
            } else {
                positioners[idx].row = mid;
            }
        }
    }

    /// Collision detection and resolution between subtrees
    fn on_finish_calculation(
        positioners: &mut Vec<AdvancementPositioner>,
        idx: usize,
        mut default_ancestor_idx: usize,
    ) -> usize {
        let prev_sibling_idx = match positioners[idx].previous_sibling_idx {
            Some(prev) => prev,
            None => return default_ancestor_idx,
        };

        let mut inside_idx = idx;
        let mut outside_idx = idx;
        let mut left_sibling_idx = prev_sibling_idx;

        let parent_idx = positioners[idx].parent_idx.unwrap();
        let mut left_ancestor_idx = positioners[parent_idx].children[0];

        let mut shift_inside = positioners[idx].relative_row_in_siblings;
        let mut shift_outside = positioners[idx].relative_row_in_siblings;
        let mut shift_left_sibling = positioners[left_sibling_idx].relative_row_in_siblings;
        let mut shift_left_ancestor = positioners[left_ancestor_idx].relative_row_in_siblings;

        loop {
            let left_last = Self::get_last_child(positioners, left_sibling_idx);
            let inside_first = Self::get_first_child(positioners, inside_idx);

            match (left_last, inside_first) {
                (Some(ll), Some(if_)) => {
                    left_sibling_idx = ll;
                    inside_idx = if_;

                    if let Some(la_first) = Self::get_first_child(positioners, left_ancestor_idx) {
                        left_ancestor_idx = la_first;
                    }
                    if let Some(out_last) = Self::get_last_child(positioners, outside_idx) {
                        outside_idx = out_last;
                    }

                    positioners[outside_idx].optional_last_idx = idx;

                    let move_distance = (positioners[left_sibling_idx].row + shift_left_sibling)
                        - (positioners[inside_idx].row + shift_inside)
                        + 1.0;

                    if move_distance > 0.0 {
                        let ancestor = Self::get_ancestor(
                            positioners,
                            left_sibling_idx,
                            idx,
                            default_ancestor_idx,
                        );
                        Self::push_down(positioners, ancestor, idx, move_distance);
                        shift_inside += move_distance;
                        shift_outside += move_distance;
                    }

                    shift_left_sibling += positioners[left_sibling_idx].relative_row_in_siblings;
                    shift_inside += positioners[inside_idx].relative_row_in_siblings;
                    shift_left_ancestor += positioners[left_ancestor_idx].relative_row_in_siblings;
                    shift_outside += positioners[outside_idx].relative_row_in_siblings;
                }
                _ => break,
            }
        }

        // Thread handling
        if Self::get_last_child(positioners, left_sibling_idx).is_some()
            && Self::get_last_child(positioners, outside_idx).is_none()
        {
            positioners[outside_idx].substitute_child_idx =
                Self::get_last_child(positioners, left_sibling_idx);
            positioners[outside_idx].relative_row_in_siblings += shift_left_sibling - shift_outside;
        } else {
            if Self::get_first_child(positioners, inside_idx).is_some()
                && Self::get_first_child(positioners, left_ancestor_idx).is_none()
            {
                positioners[left_ancestor_idx].substitute_child_idx =
                    Self::get_first_child(positioners, inside_idx);
                positioners[left_ancestor_idx].relative_row_in_siblings +=
                    shift_inside - shift_left_ancestor;
            }
            default_ancestor_idx = idx;
        }

        default_ancestor_idx
    }

    fn get_first_child(positioners: &[AdvancementPositioner], idx: usize) -> Option<usize> {
        if let Some(sub) = positioners[idx].substitute_child_idx {
            Some(sub)
        } else if !positioners[idx].children.is_empty() {
            Some(positioners[idx].children[0])
        } else {
            None
        }
    }

    fn get_last_child(positioners: &[AdvancementPositioner], idx: usize) -> Option<usize> {
        if let Some(sub) = positioners[idx].substitute_child_idx {
            Some(sub)
        } else if !positioners[idx].children.is_empty() {
            Some(*positioners[idx].children.last().unwrap())
        } else {
            None
        }
    }

    fn get_ancestor(
        positioners: &[AdvancementPositioner],
        left_sibling_idx: usize,
        current_idx: usize,
        default_ancestor_idx: usize,
    ) -> usize {
        let optional_last = positioners[left_sibling_idx].optional_last_idx;
        let parent_idx = positioners[current_idx].parent_idx.unwrap();
        let parent_children = &positioners[parent_idx].children;

        if parent_children.contains(&optional_last) {
            optional_last
        } else {
            default_ancestor_idx
        }
    }

    fn push_down(
        positioners: &mut Vec<AdvancementPositioner>,
        ancestor_idx: usize,
        current_idx: usize,
        move_distance: f32,
    ) {
        let subtrees =
            (positioners[current_idx].children_size - positioners[ancestor_idx].children_size)
                as f32;
        if subtrees != 0.0 {
            positioners[current_idx].shift_acceleration -= move_distance / subtrees;
            positioners[ancestor_idx].shift_acceleration += move_distance / subtrees;
        }
        positioners[current_idx].shift_change += move_distance;
        positioners[current_idx].row += move_distance;
        positioners[current_idx].relative_row_in_siblings += move_distance;
    }

    /// Apply accumulated shifts to children
    fn on_finish_children_calculation(positioners: &mut Vec<AdvancementPositioner>, idx: usize) {
        let children = positioners[idx].children.clone();
        let mut shift = 0.0f32;
        let mut change = 0.0f32;

        for i in (0..children.len()).rev() {
            let child_idx = children[i];
            positioners[child_idx].row += shift;
            positioners[child_idx].relative_row_in_siblings += shift;
            change += positioners[child_idx].shift_acceleration;
            shift += positioners[child_idx].shift_change + change;
        }
    }

    /// Find minimum row value in the tree
    fn find_min_row_recursive(
        positioners: &mut Vec<AdvancementPositioner>,
        idx: usize,
        delta_row: f32,
        depth: i32,
    ) -> f32 {
        positioners[idx].row += delta_row;
        positioners[idx].depth = depth;

        let mut min_row = positioners[idx].row;

        let children = positioners[idx].children.clone();
        for child_idx in children {
            let child_delta = delta_row + positioners[idx].relative_row_in_siblings;
            let child_min =
                Self::find_min_row_recursive(positioners, child_idx, child_delta, depth + 1);
            if child_min < min_row {
                min_row = child_min;
            }
        }

        min_row
    }

    /// Shift all rows by delta
    fn increase_row_recursive(
        positioners: &mut Vec<AdvancementPositioner>,
        idx: usize,
        delta: f32,
    ) {
        positioners[idx].row += delta;

        let children = positioners[idx].children.clone();
        for child_idx in children {
            Self::increase_row_recursive(positioners, child_idx, delta);
        }
    }

}

fn load_advancements_recursive(
    dir: &Path,
    namespace: &str,
    prefix: &str,
    advancements: &mut Vec<AdvancementData>,
) -> Result<(), LoadError> {
    for entry in fs::read_dir(dir).map_err(|e| LoadError::Io(e.to_string()))? {
        let entry = entry.map_err(|e| LoadError::Io(e.to_string()))?;
        let path = entry.path();

        if path.is_dir() {
            let dir_name = path.file_name().unwrap().to_string_lossy();
            let new_prefix = if prefix.is_empty() {
                dir_name.to_string()
            } else {
                format!("{prefix}/{dir_name}")
            };
            load_advancements_recursive(&path, namespace, &new_prefix, advancements)?;
        } else if path.extension().is_some_and(|ext| ext == "json") {
            let file_name = path.file_stem().unwrap().to_string_lossy();
            let advancement_id = if prefix.is_empty() {
                format!("{namespace}:{file_name}")
            } else {
                format!("{namespace}:{prefix}/{file_name}")
            };

            match load_advancement_file(&path, &advancement_id) {
                Ok(adv) => advancements.push(adv),
                Err(e) => {
                    log::warn!("Failed to load advancement {advancement_id}: {e}");
                }
            }
        }
    }
    Ok(())
}

fn load_advancement_file(path: &Path, advancement_id: &str) -> Result<AdvancementData, LoadError> {
    let content = fs::read_to_string(path).map_err(|e| LoadError::Io(e.to_string()))?;
    let json: JsonAdvancement =
        serde_json::from_str(&content).map_err(|e| LoadError::Parse(e.to_string()))?;

    Ok(convert_json_advancement(json, advancement_id))
}

fn convert_json_advancement(json: JsonAdvancement, id: &str) -> AdvancementData {
    let display = json.display.map(|d| {
        let icon_item = d
            .icon
            .id
            .strip_prefix("minecraft:")
            .and_then(|name| Item::from_registry_key(name))
            .unwrap_or(&Item::AIR);

        AdvancementDisplayData {
            title: parse_text_component(&d.title),
            description: parse_text_component(&d.description),
            icon: ItemStack::new(d.icon.count.unwrap_or(1) as u8, icon_item),
            frame: d.frame.unwrap_or_default(),
            background: d.background.as_deref().map(ResourceLocation::from),
            show_toast: d.show_toast.unwrap_or(true),
            announce_to_chat: d.announce_to_chat.unwrap_or(true),
            hidden: d.hidden.unwrap_or(false),
            x: 0.0, // Will be set by calculate_layout()
            y: 0.0, // Will be set by calculate_layout()
        }
    });

    let criteria: HashMap<String, CriterionData> = json
        .criteria
        .into_iter()
        .map(|(name, crit)| {
            (
                name,
                CriterionData {
                    trigger: ResourceLocation::from(crit.trigger.as_str()),
                    conditions: crit.conditions.unwrap_or(serde_json::Value::Null),
                },
            )
        })
        .collect();

    // Requirements: if not specified, all criteria must be met (AND)
    let requirements = json.requirements.unwrap_or_else(|| {
        criteria.keys().map(|k| vec![k.clone()]).collect()
    });

    AdvancementData {
        id: ResourceLocation::from(id),
        parent: json.parent.as_deref().map(ResourceLocation::from),
        display,
        criteria,
        requirements,
        sends_telemetry_event: json.sends_telemetry_event.unwrap_or(false),
    }
}

fn parse_text_component(value: &serde_json::Value) -> TextComponent {
    match value {
        serde_json::Value::String(s) => TextComponent::text(s.clone()),
        serde_json::Value::Object(obj) => {
            // Try translate key first, then text key, then empty
            obj.get("translate")
                .and_then(|v| v.as_str())
                .map(|t| TextComponent::translate(t.to_string(), []))
                .or_else(|| {
                    obj.get("text")
                        .and_then(|v| v.as_str())
                        .map(|t| TextComponent::text(t.to_string()))
                })
                .unwrap_or_else(|| TextComponent::text(""))
        }
        _ => TextComponent::text(""),
    }
}

#[derive(Debug)]
pub enum LoadError {
    PathNotFound(String),
    Io(String),
    Parse(String),
}

impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::PathNotFound(p) => write!(f, "Path not found: {p}"),
            Self::Io(e) => write!(f, "IO error: {e}"),
            Self::Parse(e) => write!(f, "Parse error: {e}"),
        }
    }
}

// JSON structures matching vanilla advancement format

#[derive(Deserialize)]
struct JsonAdvancement {
    parent: Option<String>,
    display: Option<JsonDisplay>,
    criteria: HashMap<String, JsonCriterion>,
    requirements: Option<Vec<Vec<String>>>,
    sends_telemetry_event: Option<bool>,
}

#[derive(Deserialize)]
struct JsonDisplay {
    icon: JsonIcon,
    title: serde_json::Value,
    description: serde_json::Value,
    frame: Option<AdvancementFrame>,
    background: Option<String>,
    show_toast: Option<bool>,
    announce_to_chat: Option<bool>,
    hidden: Option<bool>,
}

#[derive(Deserialize)]
struct JsonIcon {
    id: String,
    count: Option<i32>,
}

#[derive(Deserialize)]
struct JsonCriterion {
    trigger: String,
    conditions: Option<serde_json::Value>,
}
