use crate::WritingError;
use crate::codec::bit_set::BitSet;
use crate::{ClientPacket, VarInt, ser::NetworkWriteExt};
use pumpkin_data::block_state_remap::remap_block_state_for_version;
use pumpkin_data::packet::clientbound::PLAY_LEVEL_CHUNK_WITH_LIGHT;
use pumpkin_macros::java_packet;
use pumpkin_nbt::END_ID;
use pumpkin_util::math::position::get_local_cord;
use pumpkin_util::version::MinecraftVersion;
use pumpkin_world::chunk::format::LightContainer;
use pumpkin_world::chunk::{ChunkData, palette::NetworkPalette};
use std::io::Write;

/// Sent by the server to provide the client with the full data for a chunk.
///
/// This includes heightmaps, the actual block and biome data (organized into sections),
/// block entities (like signs or chests), and the light level information for both
/// sky and block light.
#[java_packet(PLAY_LEVEL_CHUNK_WITH_LIGHT)]
pub struct CChunkData<'a>(pub &'a ChunkData);

impl CChunkData<'_> {
    #[expect(clippy::too_many_lines)]
    fn serialize_packet_data(&self, version: &MinecraftVersion) -> Result<Vec<u8>, WritingError> {
        let mut buf = Vec::with_capacity(49152);

        // Chunk X
        buf.write_i32_be(self.0.x)?;
        // Chunk Z
        buf.write_i32_be(self.0.z)?;

        let heightmaps = &self.0.heightmap.lock().unwrap();
        buf.write_var_int(&VarInt(3))?; // Map size

        let write_heightmap =
            |buf: &mut Vec<u8>, index: i32, data: &[i64]| -> Result<(), WritingError> {
                buf.write_var_int(&VarInt(index))?;
                buf.write_var_int(&VarInt(data.len() as i32))?;
                for val in data {
                    buf.write_i64_be(*val)?;
                }
                Ok(())
            };

        write_heightmap(&mut buf, 1, &heightmaps.world_surface)?;
        write_heightmap(&mut buf, 4, &heightmaps.motion_blocking)?;
        write_heightmap(&mut buf, 5, &heightmaps.motion_blocking_no_leaves)?;

        {
            let mut blocks_and_biomes_buf = Vec::with_capacity(40960);

            for (block_lock, biome_lock) in self
                .0
                .section
                .block_sections
                .iter()
                .zip(self.0.section.biome_sections.iter())
            {
                let block_palette = block_lock.read().unwrap();
                let biome_palette = biome_lock.read().unwrap();
                let non_empty_block_count = block_palette.non_air_block_count() as i16;
                blocks_and_biomes_buf.write_i16_be(non_empty_block_count)?;

                let mut block_network = block_palette.convert_network();
                match &mut block_network.palette {
                    NetworkPalette::Single(registry_id) => {
                        *registry_id = remap_block_state_for_version(*registry_id, *version);
                    }
                    NetworkPalette::Indirect(palette) => {
                        for registry_id in palette.iter_mut() {
                            *registry_id = remap_block_state_for_version(*registry_id, *version);
                        }
                    }
                    NetworkPalette::Direct => {
                        let bits_per_entry = usize::from(block_network.bits_per_entry);
                        let values_per_i64 = 64 / bits_per_entry;
                        let id_mask = (1u64 << bits_per_entry) - 1;

                        for packed_word in &mut block_network.packed_data {
                            let mut remapped_word = 0u64;
                            let packed_word_u64 = *packed_word as u64;
                            for index in 0..values_per_i64 {
                                let shift = index * bits_per_entry;
                                let state_id = ((packed_word_u64 >> shift) & id_mask) as u16;
                                let remapped_id = remap_block_state_for_version(state_id, *version);
                                remapped_word |= u64::from(remapped_id) << shift;
                            }
                            *packed_word = remapped_word as i64;
                        }
                    }
                }
                blocks_and_biomes_buf.write_u8(block_network.bits_per_entry)?;

                match block_network.palette {
                    NetworkPalette::Single(registry_id) => {
                        blocks_and_biomes_buf.write_var_int(&registry_id.into())?;
                    }
                    NetworkPalette::Indirect(palette) => {
                        blocks_and_biomes_buf.write_var_int(&palette.len().try_into().map_err(
                            |_| {
                                WritingError::Message(format!(
                                    "{} is not representable as a VarInt!",
                                    palette.len()
                                ))
                            },
                        )?)?;
                        for registry_id in palette {
                            blocks_and_biomes_buf.write_var_int(&registry_id.into())?;
                        }
                    }
                    NetworkPalette::Direct => {}
                }

                for packed in block_network.packed_data {
                    blocks_and_biomes_buf.write_i64_be(packed)?;
                }

                let biome_network = biome_palette.convert_network();
                blocks_and_biomes_buf.write_u8(biome_network.bits_per_entry)?;

                match biome_network.palette {
                    NetworkPalette::Single(registry_id) => {
                        blocks_and_biomes_buf.write_var_int(&registry_id.into())?;
                    }
                    NetworkPalette::Indirect(palette) => {
                        blocks_and_biomes_buf.write_var_int(&palette.len().try_into().map_err(
                            |_| {
                                WritingError::Message(format!(
                                    "{} is not representable as a VarInt!",
                                    palette.len()
                                ))
                            },
                        )?)?;
                        for registry_id in palette {
                            blocks_and_biomes_buf.write_var_int(&registry_id.into())?;
                        }
                    }
                    NetworkPalette::Direct => {}
                }

                for packed in biome_network.packed_data {
                    blocks_and_biomes_buf.write_i64_be(packed)?;
                }
            }

            buf.write_var_int(&blocks_and_biomes_buf.len().try_into().map_err(|_| {
                WritingError::Message(format!(
                    "{} is not representable as a VarInt!",
                    blocks_and_biomes_buf.len()
                ))
            })?)?;
            buf.write_slice(&blocks_and_biomes_buf)?;
        };

        let block_entities = self.0.block_entities.lock().unwrap();
        buf.write_var_int(&VarInt(block_entities.len() as i32))?;
        for block_entity in block_entities.values() {
            let pos = block_entity.get_position();
            let local_xz = ((get_local_cord(pos.0.x) & 0xF) << 4) | (get_local_cord(pos.0.z) & 0xF);

            buf.write_u8(local_xz as u8)?;
            buf.write_i16_be(pos.0.y as i16)?;
            buf.write_var_int(&VarInt(block_entity.get_id() as i32))?;

            if let Some(nbt) = block_entity.chunk_data_nbt() {
                buf.write_nbt(nbt.into())?;
            } else {
                buf.write_u8(END_ID)?;
            }
        }

        {
            // Light masks include sections from -1 (below world) to num_sections (above world)
            // This means we need to account for 2 extra sections in the bitset
            let light_engine = self.0.light_engine.lock().unwrap();
            let num_sections = light_engine.sky_light.len();

            let mut sky_light_empty_mask = 0u64;
            let mut block_light_empty_mask = 0u64;
            let mut sky_light_mask = 0u64;
            let mut block_light_mask = 0u64;

            // Bit 0 represents the section below the world (always empty)
            sky_light_empty_mask |= 1 << 0;
            block_light_empty_mask |= 1 << 0;

            // Bits 1..=num_sections represent the actual world sections
            for section_index in 0..num_sections {
                let bit_index = section_index + 1; // Offset by 1 for the below-world section

                if let LightContainer::Full(_) = &light_engine.sky_light[section_index] {
                    sky_light_mask |= 1 << bit_index;
                } else {
                    sky_light_empty_mask |= 1 << bit_index;
                }

                if let LightContainer::Full(_) = &light_engine.block_light[section_index] {
                    block_light_mask |= 1 << bit_index;
                } else {
                    block_light_empty_mask |= 1 << bit_index;
                }
            }

            // Bit num_sections+1 represents the section above the world (always empty)
            sky_light_empty_mask |= 1 << (num_sections + 1);
            block_light_empty_mask |= 1 << (num_sections + 1);

            // Write Sky Light Mask
            buf.write_bitset(&BitSet(Box::new([sky_light_mask.try_into().unwrap()])))?;
            // Write Block Light Mask
            buf.write_bitset(&BitSet(Box::new([block_light_mask.try_into().unwrap()])))?;
            // Write Empty Sky Light Mask
            buf.write_bitset(&BitSet(Box::new([sky_light_empty_mask
                .try_into()
                .unwrap()])))?;
            // Write Empty Block Light Mask
            buf.write_bitset(&BitSet(Box::new([block_light_empty_mask
                .try_into()
                .unwrap()])))?;

            let light_data_size: VarInt = LightContainer::ARRAY_SIZE.try_into().unwrap();

            // Write Sky Light arrays
            buf.write_var_int(&VarInt(sky_light_mask.count_ones() as i32))?;
            for section_index in 0..num_sections {
                if let LightContainer::Full(data) = &light_engine.sky_light[section_index] {
                    buf.write_var_int(&light_data_size)?;
                    buf.write_slice(data.as_ref())?;
                }
            }

            // Write Block Light arrays
            buf.write_var_int(&VarInt(block_light_mask.count_ones() as i32))?;
            for section_index in 0..num_sections {
                if let LightContainer::Full(data) = &light_engine.block_light[section_index] {
                    buf.write_var_int(&light_data_size)?;
                    buf.write_slice(data.as_ref())?;
                }
            }
        }
        Ok(buf)
    }
}

impl ClientPacket for CChunkData<'_> {
    fn write_packet_data(
        &self,
        write: impl Write,
        version: &MinecraftVersion,
    ) -> Result<(), WritingError> {
        let mut write = write;

        {
            let cache = self.0.network_cache.lock().unwrap();
            if let Some(cached_bytes) = cache.get(version) {
                write.write_slice(cached_bytes)?;
                return Ok(());
            }
        }

        let serialized = self.serialize_packet_data(version)?;
        write.write_slice(&serialized)?;

        self.0
            .network_cache
            .lock()
            .unwrap()
            .insert(*version, serialized.into());

        Ok(())
    }
}
