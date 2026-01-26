use std::io::Write;

use pumpkin_data::packet::clientbound::PLAY_UPDATE_ADVANCEMENTS;
use pumpkin_macros::java_packet;
use pumpkin_nbt::serializer::Serializer as NbtSerializer;
use pumpkin_util::{resource_location::ResourceLocation, text::TextComponent};
use pumpkin_util::version::MinecraftVersion;
use serde::Serialize;

use crate::{ClientPacket, WritingError, ser::NetworkWriteExt};
use crate::codec::item_stack_seralizer::ItemStackSerializer;
use crate::ser::serializer::Serializer as ProtocolSerializer;

/// Updates advancement progress and unlocked advancements for the client.
///
/// This packet is sent when the player first joins and whenever their
/// advancement progress changes. It can add new advancements, remove old ones,
/// and update progress on existing advancements.
#[java_packet(PLAY_UPDATE_ADVANCEMENTS)]
pub struct CUpdateAdvancements<'a> {
    /// Whether to reset/clear the current advancements before applying updates.
    /// Set to true on initial join, false for incremental updates.
    pub reset: bool,
    /// New advancements to add to the client's advancement tree.
    pub advancements: &'a [AdvancementMapping<'a>],
    /// Identifiers of advancements to remove from the client.
    pub remove_identifiers: &'a [ResourceLocation],
    /// Progress updates for advancements.
    pub progress: &'a [AdvancementProgressMapping<'a>],
    /// Whether to show toast notifications for completed advancements.
    pub show_toast: bool,
}

impl<'a> CUpdateAdvancements<'a> {
    pub fn new(
        reset: bool,
        advancements: &'a [AdvancementMapping<'a>],
        remove_identifiers: &'a [ResourceLocation],
        progress: &'a [AdvancementProgressMapping<'a>],
        show_toast: bool,
    ) -> Self {
        Self {
            reset,
            advancements,
            remove_identifiers,
            progress,
            show_toast,
        }
    }
}

impl ClientPacket for CUpdateAdvancements<'_> {
    fn write_packet_data(
        &self,
        write: impl Write,
        _version: &MinecraftVersion,
    ) -> Result<(), WritingError> {
        let mut write = write;

        // Reset/clear flag
        write.write_bool(self.reset)?;

        // Advancement mapping array (List<AdvancementEntry>)
        write.write_list(self.advancements, |w, adv| {
            // Write the identifier (key)
            w.write_resource_location(&adv.id)?;
            // Write the advancement data (value)
            adv.advancement.write_to(w)
        })?;

        // Identifiers to remove (Set<Identifier>)
        write.write_list(self.remove_identifiers, |w, id| {
            w.write_resource_location(id)
        })?;

        // Progress mapping array (Map<Identifier, AdvancementProgress>)
        write.write_list(self.progress, |w, prog| {
            w.write_resource_location(&prog.id)?;
            prog.progress.write_to(w)
        })?;

        // Show toast boolean (at the end!)
        write.write_bool(self.show_toast)?;

        Ok(())
    }
}

/// A mapping of advancement identifier to advancement data.
pub struct AdvancementMapping<'a> {
    pub id: ResourceLocation,
    pub advancement: Advancement<'a>,
}

/// Represents a single advancement in the advancement tree.
pub struct Advancement<'a> {
    /// The parent advancement's identifier, if this isn't a root advancement.
    pub parent: Option<ResourceLocation>,
    /// Display information for the advancement, if it should be shown in the GUI.
    pub display: Option<AdvancementDisplay<'a>>,
    /// Requirements are groups of criteria that must all be completed.
    /// The advancement is complete when ALL requirement groups have at least one criterion complete.
    /// Each inner array is an OR group, outer array is AND.
    pub requirements: &'a [&'a [&'a str]],
    /// Whether to send telemetry data when this advancement is completed.
    pub sends_telemetry_event: bool,
}

impl Advancement<'_> {
    fn write_to(&self, write: &mut impl Write) -> Result<(), WritingError> {
        // Parent (optional)
        write.write_option(&self.parent, |w, parent| {
            w.write_resource_location(parent)
        })?;

        // Display (optional)
        write.write_option(&self.display, |w, display| {
            display.write_to(w)
        })?;

        // Requirements - array of arrays of criterion names (no criteria field!)
        write.write_list(self.requirements, |w, requirement| {
            w.write_list(requirement, |w2, criterion_name| {
                w2.write_string(criterion_name)
            })
        })?;

        // Sends telemetry event
        write.write_bool(self.sends_telemetry_event)?;

        Ok(())
    }
}

/// Display information for an advancement shown in the GUI.
pub struct AdvancementDisplay<'a> {
    /// The title shown in the advancement GUI.
    pub title: &'a TextComponent,
    /// The description shown when hovering over the advancement.
    pub description: &'a TextComponent,
    /// The item displayed as the icon for this advancement.
    pub icon: &'a ItemStackSerializer<'a>,
    /// The type of frame around the advancement icon.
    pub frame_type: AdvancementFrameType,
    /// Bitfield flags for display options.
    /// 0x1 = has background texture
    /// 0x2 = show toast notification
    /// 0x4 = hidden (only show when completed or has completed child)
    pub flags: i32,
    /// Background texture location (only present if flags & 0x1).
    pub background_texture: Option<ResourceLocation>,
    /// X position in the advancement tab.
    pub x: f32,
    /// Y position in the advancement tab.
    pub y: f32,
}

impl AdvancementDisplay<'_> {
    fn write_to(&self, write: &mut impl Write) -> Result<(), WritingError> {
        // Title (Chat/TextComponent as NBT)
        {
            let translated = self.title.0.clone().to_translated();
            let mut nbt_serializer = NbtSerializer::new(write.by_ref(), None);
            translated.serialize(&mut nbt_serializer).map_err(|err| {
                WritingError::Serde(format!("Failed to serialize TextComponent NBT: {err}"))
            })?;
        }

        // Description (Chat/TextComponent as NBT)
        {
            let translated = self.description.0.clone().to_translated();
            let mut nbt_serializer = NbtSerializer::new(write.by_ref(), None);
            translated.serialize(&mut nbt_serializer).map_err(|err| {
                WritingError::Serde(format!("Failed to serialize TextComponent NBT: {err}"))
            })?;
        }

        // Icon (Slot/ItemStack) - use the protocol serializer
        {
            let mut protocol_serializer = ProtocolSerializer::new(write.by_ref());
            self.icon.serialize(&mut protocol_serializer)?;
        }

        // Frame type (enum constant as VarInt)
        write.write_var_int(&(self.frame_type as i32).into())?;

        // Flags (int, NOT VarInt)
        write.write_i32_be(self.flags)?;

        // Background texture (only if flags & 0x1)
        if self.flags & 0x1 != 0
            && let Some(ref bg) = self.background_texture
        {
            write.write_resource_location(bg)?;
        }

        // X and Y coordinates
        write.write_f32_be(self.x)?;
        write.write_f32_be(self.y)?;

        Ok(())
    }
}

/// The frame type determines how the advancement icon is displayed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(i32)]
pub enum AdvancementFrameType {
    /// Normal advancement (square frame)
    Task = 0,
    /// Challenge advancement (spiked frame, plays special sound)
    Challenge = 1,
    /// Goal advancement (rounded frame)
    Goal = 2,
}

/// A mapping of advancement identifier to progress data.
pub struct AdvancementProgressMapping<'a> {
    pub id: ResourceLocation,
    pub progress: AdvancementProgress<'a>,
}

/// Progress data for a single advancement.
pub struct AdvancementProgress<'a> {
    /// Progress for each criterion in the advancement.
    pub criteria: &'a [CriterionProgressMapping<'a>],
}

impl AdvancementProgress<'_> {
    fn write_to(&self, write: &mut impl Write) -> Result<(), WritingError> {
        // Map<String, CriterionProgress>
        write.write_list(self.criteria, |w, criterion| {
            w.write_string(criterion.criterion)?;
            criterion.progress.write_to(w)
        })
    }
}

/// A mapping of criterion name to its progress.
pub struct CriterionProgressMapping<'a> {
    pub criterion: &'a str,
    pub progress: CriterionProgress,
}

/// Progress data for a single criterion.
///
/// The format is: Optional<Instant> where Instant is written as i64 (epoch millis).
/// Written as: boolean (present), then if present, i64 (millis).
pub struct CriterionProgress {
    /// The time when this criterion was achieved (as Unix timestamp in milliseconds).
    /// None if not yet achieved.
    pub obtained_time: Option<i64>,
}

impl CriterionProgress {
    fn write_to(&self, write: &mut impl Write) -> Result<(), WritingError> {
        // writeNullable format: boolean + optional value
        match self.obtained_time {
            Some(time) => {
                write.write_bool(true)?;
                write.write_i64_be(time)?;
            }
            None => {
                write.write_bool(false)?;
            }
        }
        Ok(())
    }
}
