use std::sync::OnceLock;

use crate::block::BlockState;

pub const SECTION_VOLUME: usize = 16 * 16 * 16;
const MAX_PALETTE: usize = 4096;

static AIR: OnceLock<BlockState> = OnceLock::new();

fn air() -> &'static BlockState {
    AIR.get_or_init(BlockState::default)
}

/// A 16×16×16 section using a palette plus bit-packed indices.
///
/// Empty sections have a one-entry palette and no index buffer. Sections with a single
/// state also omit the index buffer, which is the common case for solid terrain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
    palette: Vec<BlockState>,
    /// Packed indices, `bits_per_index` per entry. Empty when `palette.len() == 1`.
    data: Vec<u64>,
    bits_per_index: u8,
    non_air: u16,
}

impl Default for Section {
    fn default() -> Self {
        Self::empty()
    }
}

impl Section {
    pub fn empty() -> Self {
        Self {
            palette: vec![air().clone()],
            data: Vec::new(),
            bits_per_index: 0,
            non_air: 0,
        }
    }

    pub fn single(state: BlockState) -> Self {
        let non_air = if state.is_air() {
            0
        } else {
            SECTION_VOLUME as u16
        };
        Self {
            palette: vec![state],
            data: Vec::new(),
            bits_per_index: 0,
            non_air,
        }
    }

    pub fn palette(&self) -> &[BlockState] {
        &self.palette
    }

    pub fn non_air_count(&self) -> usize {
        self.non_air as usize
    }

    pub fn is_empty(&self) -> bool {
        self.non_air == 0
    }

    pub fn palette_len(&self) -> usize {
        self.palette.len()
    }

    pub fn bits_per_index(&self) -> u8 {
        self.bits_per_index
    }

    #[inline]
    pub fn local_index(x: u8, y: u8, z: u8) -> Option<usize> {
        if x >= 16 || y >= 16 || z >= 16 {
            return None;
        }
        Some(((y as usize) * 16 + z as usize) * 16 + x as usize)
    }

    pub fn get(&self, x: u8, y: u8, z: u8) -> Option<&BlockState> {
        let index = Self::local_index(x, y, z)?;
        if self.palette.len() == 1 {
            return self.palette.first();
        }
        let palette_index = self.read_index(index) as usize;
        self.palette.get(palette_index)
    }

    /// Sets a local block. Returns true when the state actually changed.
    pub fn set(&mut self, x: u8, y: u8, z: u8, state: &BlockState) -> Option<bool> {
        let index = Self::local_index(x, y, z)?;
        let previous_air = self.get(x, y, z)?.is_air();
        let next_air = state.is_air();

        if self.palette.len() == 1 {
            if self.palette[0] == *state {
                return Some(false);
            }
            let previous = self.palette[0].clone();
            self.palette = vec![previous.clone(), state.clone()];
            self.bits_per_index = 1;
            self.data = vec![0u64; (SECTION_VOLUME).div_ceil(64)];
            self.non_air = if !previous.is_air() && next_air {
                (SECTION_VOLUME - 1) as u16
            } else if previous.is_air() && !next_air {
                1
            } else if !previous.is_air() && !next_air {
                SECTION_VOLUME as u16
            } else {
                0
            };
            self.write_index(index, 1);
            return Some(true);
        }

        let (palette_index, is_new) = match self.palette.iter().position(|entry| entry == state) {
            Some(index) => (index, false),
            None => {
                if self.palette.len() >= MAX_PALETTE {
                    return None;
                }
                self.palette.push(state.clone());
                (self.palette.len() - 1, true)
            }
        };
        if is_new {
            self.ensure_capacity();
        }
        if self.read_index(index) as usize != palette_index {
            self.write_index(index, palette_index as u16);
            if previous_air && !next_air {
                self.non_air += 1;
            } else if !previous_air && next_air {
                self.non_air -= 1;
            }
            if self.non_air == 0 {
                self.reset_to_air();
            }
            Some(true)
        } else {
            Some(false)
        }
    }

    fn ensure_capacity(&mut self) {
        let needed_bits = bits_for(self.palette.len().max(2));
        if needed_bits <= self.bits_per_index {
            return;
        }
        let old_bits = self.bits_per_index;
        let old_data = std::mem::take(&mut self.data);
        self.bits_per_index = needed_bits;
        self.data = vec![0u64; (SECTION_VOLUME * needed_bits as usize).div_ceil(64)];
        if old_bits == 0 {
            return;
        }
        for index in 0..SECTION_VOLUME {
            let value = read_packed(&old_data, old_bits, index);
            write_packed(&mut self.data, needed_bits, index, value);
        }
    }

    fn reset_to_air(&mut self) {
        self.palette = vec![air().clone()];
        self.data.clear();
        self.bits_per_index = 0;
        self.non_air = 0;
    }

    fn read_index(&self, index: usize) -> u16 {
        read_packed(&self.data, self.bits_per_index, index)
    }

    fn write_index(&mut self, index: usize, value: u16) {
        write_packed(&mut self.data, self.bits_per_index, index, value);
    }
}

fn bits_for(palette_len: usize) -> u8 {
    debug_assert!(palette_len >= 2);
    (usize::BITS - (palette_len - 1).leading_zeros()) as u8
}

fn read_packed(data: &[u64], bits: u8, index: usize) -> u16 {
    if bits == 0 {
        return 0;
    }
    let bit = index * bits as usize;
    let word = bit / 64;
    let offset = bit % 64;
    let mask = (1u64 << bits) - 1;
    let mut value = data.get(word).copied().unwrap_or(0) >> offset;
    if offset + bits as usize > 64 {
        if let Some(next) = data.get(word + 1) {
            value |= next << (64 - offset);
        }
    }
    (value & mask) as u16
}

fn write_packed(data: &mut [u64], bits: u8, index: usize, value: u16) {
    if bits == 0 {
        return;
    }
    let bit = index * bits as usize;
    let word = bit / 64;
    let offset = bit % 64;
    let mask = ((1u64 << bits) - 1) << offset;
    if let Some(entry) = data.get_mut(word) {
        *entry = (*entry & !mask) | ((value as u64) << offset);
    }
    if offset + bits as usize > 64 {
        let second_offset = 64 - offset;
        let second_mask = ((1u64 << bits) - 1) >> second_offset;
        if let Some(entry) = data.get_mut(word + 1) {
            *entry = (*entry & !second_mask) | ((value as u64) >> second_offset);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stone() -> BlockState {
        "minecraft:stone".parse().unwrap()
    }

    #[test]
    fn empty_section_has_no_index_buffer() {
        let section = Section::empty();
        assert_eq!(section.palette_len(), 1);
        assert!(section.data.is_empty());
        assert_eq!(section.bits_per_index(), 0);
        assert_eq!(section.get(0, 0, 0), Some(&BlockState::default()));
        assert!(section.is_empty());
    }

    #[test]
    fn single_solid_state_compresses_to_one_palette_entry() {
        let section = Section::single(stone());
        assert_eq!(section.palette_len(), 1);
        assert!(section.data.is_empty());
        assert!(section.data.capacity() == 0 || section.data.is_empty());
        assert_eq!(section.non_air_count(), SECTION_VOLUME);
        assert_eq!(section.get(15, 15, 15), Some(&stone()));
    }

    #[test]
    fn set_get_roundtrip_across_palette_growth() {
        let mut section = Section::empty();
        for x in 0..16 {
            let state: BlockState = "minecraft:stone".parse().unwrap();
            assert_eq!(section.set(x as u8, 0, 0, &state), Some(true));
        }
        assert_eq!(section.non_air_count(), 16);
        assert_eq!(section.get(3, 0, 0), Some(&stone()));

        // Promote to many palette entries to force bit growth and repacking.
        for (value, name) in [
            "minecraft:dirt",
            "minecraft:grass_block",
            "minecraft:sand",
            "minecraft:gravel",
            "minecraft:oak_planks",
            "minecraft:glass",
            "minecraft:gold_block",
            "minecraft:iron_block",
            "minecraft:emerald_block",
        ]
        .iter()
        .enumerate()
        {
            let state: BlockState = name.parse().unwrap();
            assert_eq!(section.set(value as u8, 1, 0, &state), Some(true));
            assert_eq!(section.get(value as u8, 1, 0), Some(&state));
        }
        assert!(section.bits_per_index() >= 4);
        assert_eq!(section.get(0, 0, 0), Some(&stone()));
        assert_eq!(section.get(3, 0, 0), Some(&stone()));
    }

    #[test]
    fn removing_all_blocks_resets_section_to_air() {
        let mut section = Section::empty();
        section.set(1, 2, 3, &stone());
        assert!(!section.is_empty());
        assert_eq!(section.set(1, 2, 3, &BlockState::default()), Some(true));
        assert!(section.is_empty());
        assert_eq!(section.palette_len(), 1);
        assert!(section.data.is_empty());
    }

    #[test]
    fn bits_for_palette_sizes() {
        assert_eq!(bits_for(2), 1);
        assert_eq!(bits_for(3), 2);
        assert_eq!(bits_for(4), 2);
        assert_eq!(bits_for(5), 3);
        assert_eq!(bits_for(256), 8);
        assert_eq!(bits_for(257), 9);
        assert_eq!(bits_for(4096), 12);
    }

    #[test]
    fn packed_indices_span_word_boundaries() {
        // 12 bits per value: values start at bit 60 and cross a u64 boundary.
        for bits in [1u8, 2, 3, 5, 8, 12] {
            let words = (SECTION_VOLUME * bits as usize).div_ceil(64);
            let mask = (1u16 << bits) - 1;
            let mut data = vec![0u64; words];
            for index in 0..SECTION_VOLUME {
                write_packed(&mut data, bits, index, (index as u16) & mask);
            }
            for index in 0..SECTION_VOLUME {
                assert_eq!(read_packed(&data, bits, index), (index as u16) & mask);
            }
        }
    }
}
