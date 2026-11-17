use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::coords::BlockPos;

/// Inclusive axis-aligned block box. Empty boxes are not representable; `min` must be
/// component-wise less than or equal to `max`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlockBox {
    pub min: BlockPos,
    pub max: BlockPos,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum BoundsError {
    #[error("minimum corner must not exceed maximum corner")]
    Inverted,
    #[error("bounds volume {volume} exceeds limit {limit}")]
    TooLarge { volume: u64, limit: u64 },
}

impl BlockBox {
    pub fn new(min: BlockPos, max: BlockPos) -> Result<Self, BoundsError> {
        if min.x > max.x || min.y > max.y || min.z > max.z {
            return Err(BoundsError::Inverted);
        }
        Ok(Self { min, max })
    }

    pub fn from_min_size(origin: BlockPos, size: [u32; 3]) -> Result<Self, BoundsError> {
        if size[0] == 0 || size[1] == 0 || size[2] == 0 {
            return Err(BoundsError::Inverted);
        }
        let max = BlockPos::new(
            origin.x + size[0] as i32 - 1,
            origin.y + size[1] as i32 - 1,
            origin.z + size[2] as i32 - 1,
        );
        Self::new(origin, max)
    }

    /// A single block.
    pub const fn single(pos: BlockPos) -> Self {
        Self { min: pos, max: pos }
    }

    pub fn volume(&self) -> u64 {
        (self.max.x - self.min.x + 1) as u64
            * (self.max.y - self.min.y + 1) as u64
            * (self.max.z - self.min.z + 1) as u64
    }

    pub fn size(&self) -> [u32; 3] {
        [
            (self.max.x - self.min.x + 1) as u32,
            (self.max.y - self.min.y + 1) as u32,
            (self.max.z - self.min.z + 1) as u32,
        ]
    }

    pub fn limit(self, max_volume: u64) -> Result<Self, BoundsError> {
        let volume = self.volume();
        if volume > max_volume {
            return Err(BoundsError::TooLarge {
                volume,
                limit: max_volume,
            });
        }
        Ok(self)
    }

    pub fn contains(&self, pos: BlockPos) -> bool {
        pos.x >= self.min.x
            && pos.x <= self.max.x
            && pos.y >= self.min.y
            && pos.y <= self.max.y
            && pos.z >= self.min.z
            && pos.z <= self.max.z
    }

    pub fn intersects(&self, other: &Self) -> bool {
        self.min.x <= other.max.x
            && self.max.x >= other.min.x
            && self.min.y <= other.max.y
            && self.max.y >= other.min.y
            && self.min.z <= other.max.z
            && self.max.z >= other.min.z
    }

    pub fn intersection(&self, other: &Self) -> Option<Self> {
        if !self.intersects(other) {
            return None;
        }
        Some(Self {
            min: BlockPos::new(
                self.min.x.max(other.min.x),
                self.min.y.max(other.min.y),
                self.min.z.max(other.min.z),
            ),
            max: BlockPos::new(
                self.max.x.min(other.max.x),
                self.max.y.min(other.max.y),
                self.max.z.min(other.max.z),
            ),
        })
    }

    pub fn iter(&self) -> BlockBoxIter {
        BlockBoxIter {
            bounds: *self,
            next: Some(self.min),
        }
    }
}

pub struct BlockBoxIter {
    bounds: BlockBox,
    next: Option<BlockPos>,
}

impl Iterator for BlockBoxIter {
    type Item = BlockPos;

    fn next(&mut self) -> Option<Self::Item> {
        let current = self.next?;
        let mut next = current;
        if next.x < self.bounds.max.x {
            next.x += 1;
        } else if next.z < self.bounds.max.z {
            next.x = self.bounds.min.x;
            next.z += 1;
        } else if next.y < self.bounds.max.y {
            next.x = self.bounds.min.x;
            next.z = self.bounds.min.z;
            next.y += 1;
        } else {
            self.next = None;
            return Some(current);
        }
        self.next = Some(next);
        Some(current)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn volume_and_size_are_inclusive() {
        let b = BlockBox::new(BlockPos::new(0, 0, 0), BlockPos::new(2, 1, 3)).unwrap();
        assert_eq!(b.volume(), 3 * 2 * 4);
        assert_eq!(b.size(), [3, 2, 4]);
    }

    #[test]
    fn rejects_inverted_bounds() {
        assert!(BlockBox::new(BlockPos::new(1, 0, 0), BlockPos::new(0, 0, 0)).is_err());
    }

    #[test]
    fn iterator_visits_every_position_once() {
        let b = BlockBox::new(BlockPos::new(-1, 4, 7), BlockPos::new(0, 5, 8)).unwrap();
        let positions: Vec<_> = b.iter().collect();
        assert_eq!(positions.len(), 8);
        assert_eq!(positions[0], BlockPos::new(-1, 4, 7));
        assert_eq!(positions[7], BlockPos::new(0, 5, 8));
        let unique: std::collections::BTreeSet<_> = positions.iter().copied().collect();
        assert_eq!(unique.len(), 8);
    }

    #[test]
    fn intersection_and_limits() {
        let a = BlockBox::new(BlockPos::new(0, 0, 0), BlockPos::new(10, 10, 10)).unwrap();
        let b = BlockBox::new(BlockPos::new(5, 5, 5), BlockPos::new(20, 20, 20)).unwrap();
        let c = a.intersection(&b).unwrap();
        assert_eq!(
            c,
            BlockBox::new(BlockPos::new(5, 5, 5), BlockPos::new(10, 10, 10)).unwrap()
        );
        assert!(a.limit(100).is_err());
        assert!(a.limit(11 * 11 * 11).is_ok());
    }
}
