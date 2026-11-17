use std::collections::BTreeMap;

use crate::block::BlockState;
use crate::coords::ChunkPos;
use crate::section::Section;

/// A 16×16 column of sections, stored sparsely by section Y.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chunk {
    pub pos: ChunkPos,
    sections: BTreeMap<i32, Section>,
}

impl Chunk {
    pub fn new(pos: ChunkPos) -> Self {
        Self {
            pos,
            sections: BTreeMap::new(),
        }
    }

    pub fn sections(&self) -> &BTreeMap<i32, Section> {
        &self.sections
    }

    pub fn section(&self, section_y: i32) -> Option<&Section> {
        self.sections.get(&section_y)
    }

    pub fn section_mut(&mut self, section_y: i32) -> Option<&mut Section> {
        self.sections.get_mut(&section_y)
    }

    pub fn ensure_section(&mut self, section_y: i32) -> &mut Section {
        self.sections.entry(section_y).or_default()
    }

    pub fn install_section(&mut self, section_y: i32, section: Option<Section>) {
        match section {
            Some(section) if !section.is_empty() => {
                self.sections.insert(section_y, section);
            }
            _ => {
                self.sections.remove(&section_y);
            }
        }
    }

    pub fn is_empty(&self) -> bool {
        self.sections.is_empty()
    }

    pub fn block_count(&self) -> usize {
        self.sections.values().map(Section::non_air_count).sum()
    }

    pub fn get_local(&self, local_x: u8, local_z: u8, y: i32) -> BlockState {
        let section_y = y.div_euclid(16);
        let local_y = y.rem_euclid(16) as u8;
        self.sections
            .get(&section_y)
            .and_then(|section| section.get(local_x, local_y, local_z))
            .cloned()
            .unwrap_or_default()
    }

    /// Sets a block using a global Y. Returns true when the state changed.
    pub fn set_local(&mut self, local_x: u8, local_z: u8, y: i32, state: &BlockState) -> bool {
        let section_y = y.div_euclid(16);
        let local_y = y.rem_euclid(16) as u8;
        if state.is_air() {
            let Some(section) = self.sections.get_mut(&section_y) else {
                return false;
            };
            let changed = section
                .set(local_x, local_y, local_z, state)
                .unwrap_or(false);
            if section.is_empty() {
                self.sections.remove(&section_y);
            }
            return changed;
        }
        self.sections
            .entry(section_y)
            .or_default()
            .set(local_x, local_y, local_z, state)
            .unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stone() -> BlockState {
        "minecraft:stone".parse().unwrap()
    }

    #[test]
    fn empty_chunk_reads_as_air() {
        let chunk = Chunk::new(ChunkPos::new(0, 0));
        assert!(chunk.is_empty());
        assert!(chunk.get_local(0, 0, 0).is_air());
        assert!(chunk.get_local(15, 15, 319).is_air());
    }

    #[test]
    fn negative_y_uses_floor_section_index() {
        let mut chunk = Chunk::new(ChunkPos::new(0, 0));
        assert!(chunk.set_local(0, 0, -1, &stone()));
        assert!(chunk.section(-1).is_some());
        assert_eq!(chunk.get_local(0, 0, -1), stone());
        assert!(chunk.get_local(0, 0, 0).is_air());
    }

    #[test]
    fn removing_last_block_drops_section() {
        let mut chunk = Chunk::new(ChunkPos::new(0, 0));
        chunk.set_local(5, 5, 64, &stone());
        assert_eq!(chunk.sections().len(), 1);
        assert!(chunk.set_local(5, 5, 64, &BlockState::default()));
        assert!(chunk.sections().is_empty());
    }
}
