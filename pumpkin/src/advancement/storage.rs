//! Persistence for player advancement progress.
//!
//! Saves and loads advancement progress in vanilla-compatible JSON format.
//! Files are stored at `world/advancements/<uuid>.json`.

use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use pumpkin_util::resource_location::ResourceLocation;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;
use uuid::Uuid;

use super::AdvancementProgressData;

/// JSON format for a single advancement's progress (vanilla compatible).
#[derive(Debug, Serialize, Deserialize)]
struct JsonAdvancementProgress {
    /// Criteria that have been obtained (name -> ISO date string).
    criteria: HashMap<String, String>,
    /// Whether the advancement is complete.
    done: bool,
}

/// JSON format for the entire progress file (vanilla compatible).
#[derive(Debug, Serialize, Deserialize, Default)]
struct JsonProgressFile {
    #[serde(flatten)]
    advancements: HashMap<String, JsonAdvancementProgress>,
}

/// Gets the path to a player's advancement progress file.
fn get_progress_path(world_path: &Path, player_uuid: Uuid) -> PathBuf {
    world_path
        .join("advancements")
        .join(format!("{player_uuid}.json"))
}

/// Converts a millisecond timestamp to an ISO 8601 date string.
fn millis_to_iso(millis: i64) -> String {
    OffsetDateTime::from_unix_timestamp_nanos(i128::from(millis) * 1_000_000)
        .ok()
        .and_then(|dt| dt.format(&Rfc3339).ok())
        .unwrap_or_else(|| {
            OffsetDateTime::now_utc()
                .format(&Rfc3339)
                .unwrap_or_default()
        })
}

/// Converts an ISO 8601 date string to a millisecond timestamp.
fn iso_to_millis(iso: &str) -> Option<i64> {
    OffsetDateTime::parse(iso, &Rfc3339)
        .ok()
        .map(|dt| (dt.unix_timestamp_nanos() / 1_000_000) as i64)
}

/// Loads advancement progress for a player from disk.
///
/// Returns a map of advancement ID -> progress data.
/// If the file doesn't exist or is invalid, returns an empty map.
#[must_use]
pub fn load_progress(
    world_path: &Path,
    player_uuid: Uuid,
) -> HashMap<ResourceLocation, AdvancementProgressData> {
    let path = get_progress_path(world_path, player_uuid);

    let content = match fs::read_to_string(&path) {
        Ok(c) => c,
        Err(e) => {
            if e.kind() != io::ErrorKind::NotFound {
                log::warn!("Failed to read advancement progress for {player_uuid}: {e}");
            }
            return HashMap::new();
        }
    };

    let json: JsonProgressFile = match serde_json::from_str(&content) {
        Ok(j) => j,
        Err(e) => {
            log::warn!("Failed to parse advancement progress for {player_uuid}: {e}");
            return HashMap::new();
        }
    };

    // Convert JSON format to internal format
    json.advancements
        .into_iter()
        .map(|(id, progress)| {
            let criteria = progress
                .criteria
                .into_iter()
                .map(|(name, date)| (name, iso_to_millis(&date)))
                .collect();

            (
                ResourceLocation::from(id.as_str()),
                AdvancementProgressData { criteria },
            )
        })
        .collect()
}

/// Saves advancement progress for a player to disk.
///
/// Only saves advancements that have at least one criterion obtained.
#[allow(clippy::implicit_hasher)]
pub fn save_progress(
    world_path: &Path,
    player_uuid: Uuid,
    progress: &HashMap<ResourceLocation, AdvancementProgressData>,
    requirements: &HashMap<ResourceLocation, Vec<Vec<String>>>,
) -> io::Result<()> {
    let path = get_progress_path(world_path, player_uuid);

    // Ensure directory exists
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    // Convert internal format to JSON format
    let mut json = JsonProgressFile::default();

    for (id, adv_progress) in progress {
        // Only save if any criteria have been obtained
        if !adv_progress.is_any_obtained() {
            continue;
        }

        let criteria: HashMap<String, String> = adv_progress
            .criteria
            .iter()
            .filter_map(|(name, time)| time.map(|t| (name.clone(), millis_to_iso(t))))
            .collect();

        // Check if advancement is complete
        let done = requirements
            .get(id)
            .is_some_and(|reqs| adv_progress.is_done(reqs));

        json.advancements
            .insert(id.to_string(), JsonAdvancementProgress { criteria, done });
    }

    // Write to file with pretty printing (like vanilla)
    let content = serde_json::to_string_pretty(&json)?;
    fs::write(&path, content)?;

    log::debug!("Saved advancement progress for {player_uuid}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn millis_to_iso_roundtrip() {
        let millis: i64 = 1_700_000_000_000; // Some timestamp
        let iso = millis_to_iso(millis);
        let back = iso_to_millis(&iso).unwrap();
        assert_eq!(millis, back);
    }
}
