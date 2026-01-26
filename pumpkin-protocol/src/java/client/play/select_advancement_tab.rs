use pumpkin_data::packet::clientbound::PLAY_SELECT_ADVANCEMENTS_TAB;
use pumpkin_macros::java_packet;
use pumpkin_util::resource_location::ResourceLocation;
use serde::Serialize;

/// Sent by the server to tell the client to switch to a specific advancement tab.
///
/// This packet is sent either:
/// - When the client switches tabs in the GUI (to confirm the switch)
/// - When an advancement in another tab is made (to show the new progress)
#[derive(Serialize)]
#[java_packet(PLAY_SELECT_ADVANCEMENTS_TAB)]
pub struct CSelectAdvancementTab {
    /// The identifier of the tab to switch to.
    /// If None, the client will switch to the first tab.
    pub tab_id: Option<ResourceLocation>,
}

impl CSelectAdvancementTab {
    pub fn new(tab_id: Option<ResourceLocation>) -> Self {
        Self { tab_id }
    }
}
