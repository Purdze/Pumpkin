//! Loads advancements from JSON files (datapack format).

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use pumpkin_data::item::Item;
use pumpkin_util::resource_location::ResourceLocation;
use pumpkin_util::text::TextComponent;
use pumpkin_world::item::ItemStack;
use serde::Deserialize;

use super::{AdvancementData, AdvancementDisplayData, AdvancementFrame, CriterionData};

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

/// Calculates x/y positions for advancements based on their parent-child tree structure.
/// Each root advancement creates a separate tab, so each tree starts at (0, 0).
/// Layout matches vanilla: horizontal progression with branches spreading up/down.
fn calculate_layout(advancements: &mut [AdvancementData]) {
    // Build children map
    let mut children: HashMap<ResourceLocation, Vec<usize>> = HashMap::new();
    let mut roots: Vec<usize> = Vec::new();

    for (idx, adv) in advancements.iter().enumerate() {
        if let Some(parent_id) = &adv.parent {
            children.entry(parent_id.clone()).or_default().push(idx);
        } else if adv.display.is_some() {
            // Only count as root if it has a display (creates a tab)
            roots.push(idx);
        }
    }

    // Process each tree independently - each root creates its own tab
    for root_idx in roots {
        // First pass: calculate subtree heights
        let mut heights: HashMap<usize, f32> = HashMap::new();
        calculate_heights(advancements, root_idx, &children, &mut heights);

        // Second pass: layout with centering
        layout_tree_centered(advancements, root_idx, 0.0, &children, &heights);
    }
}

/// Calculate the total height of each subtree.
fn calculate_heights(
    advancements: &[AdvancementData],
    idx: usize,
    children: &HashMap<ResourceLocation, Vec<usize>>,
    heights: &mut HashMap<usize, f32>,
) -> f32 {
    let id = &advancements[idx].id;
    let child_indices = children.get(id).cloned().unwrap_or_default();

    if child_indices.is_empty() {
        heights.insert(idx, 1.0);
        return 1.0;
    }

    let total: f32 = child_indices
        .iter()
        .map(|&child_idx| calculate_heights(advancements, child_idx, children, heights))
        .sum();

    let height = total.max(1.0);
    heights.insert(idx, height);
    height
}

/// Layout tree with children centered around the parent's y position.
/// This creates the vanilla-style layout where branches spread both up and down.
fn layout_tree_centered(
    advancements: &mut [AdvancementData],
    idx: usize,
    x: f32,
    children: &HashMap<ResourceLocation, Vec<usize>>,
    heights: &HashMap<usize, f32>,
) {
    let id = advancements[idx].id.clone();
    let child_indices = children.get(&id).cloned().unwrap_or_default();

    // Set this node's position at y=0 (children will be centered around it)
    if let Some(display) = &mut advancements[idx].display {
        display.x = x;
        // y will be set based on tree structure - root stays at 0
    }

    if child_indices.is_empty() {
        return;
    }

    // Sort children by ID for consistent ordering
    let mut sorted_children = child_indices;
    sorted_children.sort_by(|a, b| {
        let id_a = advancements[*a].id.to_string();
        let id_b = advancements[*b].id.to_string();
        id_a.cmp(&id_b)
    });

    // Calculate total height of all children
    let total_height: f32 = sorted_children
        .iter()
        .map(|&child_idx| heights.get(&child_idx).copied().unwrap_or(1.0))
        .sum();

    // Get parent's current y position
    let parent_y = advancements[idx]
        .display
        .as_ref()
        .map_or(0.0, |d| d.y);

    // Center children around parent's y
    let mut current_y = parent_y - total_height / 2.0;

    for child_idx in sorted_children {
        let child_height = heights.get(&child_idx).copied().unwrap_or(1.0);

        // Position child centered within its allocated space
        let child_y = current_y + child_height / 2.0;

        if let Some(display) = &mut advancements[child_idx].display {
            display.x = x + 1.0;
            display.y = child_y;
        }

        // Recursively layout this child's subtree
        layout_subtree(advancements, child_idx, x + 1.0, child_y, children, heights);

        current_y += child_height;
    }
}

/// Layout a subtree rooted at the given node (which already has its position set).
fn layout_subtree(
    advancements: &mut [AdvancementData],
    idx: usize,
    x: f32,
    y: f32,
    children: &HashMap<ResourceLocation, Vec<usize>>,
    heights: &HashMap<usize, f32>,
) {
    let id = advancements[idx].id.clone();
    let child_indices = children.get(&id).cloned().unwrap_or_default();

    if child_indices.is_empty() {
        return;
    }

    // Sort children
    let mut sorted_children = child_indices;
    sorted_children.sort_by(|a, b| {
        let id_a = advancements[*a].id.to_string();
        let id_b = advancements[*b].id.to_string();
        id_a.cmp(&id_b)
    });

    // Calculate total height
    let total_height: f32 = sorted_children
        .iter()
        .map(|&child_idx| heights.get(&child_idx).copied().unwrap_or(1.0))
        .sum();

    // Center children around this node's y
    let mut current_y = y - total_height / 2.0;

    for child_idx in sorted_children {
        let child_height = heights.get(&child_idx).copied().unwrap_or(1.0);
        let child_y = current_y + child_height / 2.0;

        if let Some(display) = &mut advancements[child_idx].display {
            display.x = x + 1.0;
            display.y = child_y;
        }

        layout_subtree(advancements, child_idx, x + 1.0, child_y, children, heights);

        current_y += child_height;
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
