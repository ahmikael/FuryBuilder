//! Deterministic local generation: heightmaps, terrain patches, objects and the Build DSL.
//!
//! These generators complement manual edits; they never pretend to reproduce Minecraft's
//! vanilla world generator. Every value is produced with integer arithmetic so the same
//! parameters and world coordinates give the same result on every platform.

use thiserror::Error;

/// Largest supported noise cell size in blocks.
pub const MAX_HEIGHTMAP_SCALE: u32 = 64;
/// Largest supported fractal octave count.
pub const MAX_HEIGHTMAP_OCTAVES: u32 = 6;
/// Largest supported maximum height offset.
pub const MAX_HEIGHTMAP_AMPLITUDE: u32 = 256;
/// Hard cap for one generated patch; the operation API applies its own selection budget.
pub const MAX_HEIGHTMAP_COLUMNS: u32 = 1_048_576;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HeightmapParams {
    pub seed: u64,
    /// Base noise cell size in blocks (1..=64).
    pub scale: u32,
    /// Fractal octave count (1..=6).
    pub octaves: u32,
    /// Maximum height offset above the base surface (1..=256).
    pub amplitude: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum GenerationError {
    #[error("heightmap width and depth must be at least 1, got {width}x{depth}")]
    EmptySize { width: u32, depth: u32 },
    #[error("heightmap {width}x{depth} exceeds the {limit} column limit")]
    TooManyColumns { width: u32, depth: u32, limit: u32 },
    #[error("heightmap scale must be 1..={MAX_HEIGHTMAP_SCALE}, got {0}")]
    InvalidScale(u32),
    #[error("heightmap octaves must be 1..={MAX_HEIGHTMAP_OCTAVES}, got {0}")]
    InvalidOctaves(u32),
    #[error("heightmap amplitude must be 1..={MAX_HEIGHTMAP_AMPLITUDE}, got {0}")]
    InvalidAmplitude(u32),
}

/// Row-major (`z * width + x`) height offsets in `0..=amplitude`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeightmapPatch {
    pub width: u32,
    pub depth: u32,
    pub amplitude: u32,
    pub offsets: Vec<u32>,
}

impl HeightmapPatch {
    pub fn offset(&self, x: u32, z: u32) -> Option<u32> {
        if x >= self.width || z >= self.depth {
            return None;
        }
        self.offsets
            .get((z as usize) * (self.width as usize) + (x as usize))
            .copied()
    }
}

/// Generates a bounded local heightmap patch. Coordinates are world-anchored: moving the
/// same footprint keeps the same terrain pattern instead of restarting at local zero.
pub fn generate_heightmap(
    width: u32,
    depth: u32,
    origin_x: i32,
    origin_z: i32,
    params: HeightmapParams,
) -> Result<HeightmapPatch, GenerationError> {
    if width == 0 || depth == 0 {
        return Err(GenerationError::EmptySize { width, depth });
    }
    let columns = u64::from(width) * u64::from(depth);
    if columns > u64::from(MAX_HEIGHTMAP_COLUMNS) {
        return Err(GenerationError::TooManyColumns {
            width,
            depth,
            limit: MAX_HEIGHTMAP_COLUMNS,
        });
    }
    if !(1..=MAX_HEIGHTMAP_SCALE).contains(&params.scale) {
        return Err(GenerationError::InvalidScale(params.scale));
    }
    if !(1..=MAX_HEIGHTMAP_OCTAVES).contains(&params.octaves) {
        return Err(GenerationError::InvalidOctaves(params.octaves));
    }
    if !(1..=MAX_HEIGHTMAP_AMPLITUDE).contains(&params.amplitude) {
        return Err(GenerationError::InvalidAmplitude(params.amplitude));
    }

    let mut offsets = Vec::with_capacity(columns as usize);
    for z in 0..depth {
        for x in 0..width {
            let world_x = i64::from(origin_x) + i64::from(x);
            let world_z = i64::from(origin_z) + i64::from(z);
            offsets.push(sample_fbm(world_x, world_z, params));
        }
    }
    Ok(HeightmapPatch {
        width,
        depth,
        amplitude: params.amplitude,
        offsets,
    })
}

/// Integer fBm: each octave contributes equally by weight, and the normalized value is
/// rounded once into the requested amplitude. No floating-point state is involved.
fn sample_fbm(x: i64, z: i64, params: HeightmapParams) -> u32 {
    let mut weighted = 0u64;
    let mut weights = 0u64;
    for octave in 0..params.octaves {
        let cell = (params.scale >> octave).max(1);
        let value = value_noise(params.seed, octave, x, z, cell);
        let weight = 1u64 << (params.octaves - 1 - octave);
        weighted += u64::from(value) * weight;
        weights += weight;
    }
    let normalized = (weighted + weights / 2) / weights;
    let offset = (normalized * u64::from(params.amplitude) + 32_767) / 65_535;
    (offset as u32).min(params.amplitude)
}

fn value_noise(seed: u64, octave: u32, x: i64, z: i64, cell: u32) -> u32 {
    let cell = i64::from(cell);
    let x0 = x.div_euclid(cell);
    let z0 = z.div_euclid(cell);
    let fx = fraction(x.rem_euclid(cell), cell);
    let fz = fraction(z.rem_euclid(cell), cell);
    let sx = smoothstep(fx);
    let sz = smoothstep(fz);
    let v00 = lattice_value(seed, octave, x0, z0);
    let v10 = lattice_value(seed, octave, x0 + 1, z0);
    let v01 = lattice_value(seed, octave, x0, z0 + 1);
    let v11 = lattice_value(seed, octave, x0 + 1, z0 + 1);
    let top = lerp(v00, v10, sx);
    let bottom = lerp(v01, v11, sx);
    lerp(top, bottom, sz)
}

/// Maps a 0..cell remainder to 0..=65535 without floating point.
fn fraction(remainder: i64, cell: i64) -> u64 {
    (remainder as u64) * 65_535 / (cell as u64)
}

fn smoothstep(t: u64) -> u64 {
    t * t * (3 * 65_535 - 2 * t) / (65_535 * 65_535)
}

fn lerp(a: u32, b: u32, t: u64) -> u32 {
    ((u64::from(a) * (65_535 - t) + u64::from(b) * t + 32_767) / 65_535) as u32
}

fn lattice_value(seed: u64, octave: u32, x: i64, z: i64) -> u32 {
    let mixed = splitmix64(
        seed ^ splitmix64(
            (x as u64)
                .rotate_left(17)
                .wrapping_add((z as u64).rotate_left(41))
                .wrapping_add(u64::from(octave).wrapping_mul(0x9E37_79B9_7F4A_7C15)),
        ),
    );
    (mixed >> 48) as u32
}

fn splitmix64(mut value: u64) -> u64 {
    value = value.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut result = value;
    result = (result ^ (result >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    result = (result ^ (result >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    result ^ (result >> 31)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params() -> HeightmapParams {
        HeightmapParams {
            seed: 0x5EED,
            scale: 8,
            octaves: 3,
            amplitude: 8,
        }
    }

    #[test]
    fn the_same_inputs_produce_identical_offsets() {
        let first = generate_heightmap(32, 24, -7, 11, params()).unwrap();
        let second = generate_heightmap(32, 24, -7, 11, params()).unwrap();
        assert_eq!(first, second);
        assert_eq!(first.offsets.len(), 32 * 24);
        assert_eq!(first.offset(0, 0), Some(first.offsets[0]));
        assert_eq!(first.offset(31, 23), Some(first.offsets[32 * 23 + 31]));
        assert_eq!(first.offset(32, 0), None);
    }

    #[test]
    fn seeds_and_world_origins_change_the_pattern() {
        let base = generate_heightmap(64, 64, 0, 0, params()).unwrap();
        let mut other = params();
        other.seed = 0xBAD5EED;
        let seeded = generate_heightmap(64, 64, 0, 0, other).unwrap();
        assert_ne!(base.offsets, seeded.offsets);
        let moved = generate_heightmap(64, 64, 17, -9, params()).unwrap();
        assert_ne!(base.offsets, moved.offsets);
    }

    #[test]
    fn offsets_never_exceed_the_amplitude() {
        for amplitude in [1, 2, 8, 64, 256] {
            let patch = generate_heightmap(
                48,
                48,
                5,
                -5,
                HeightmapParams {
                    amplitude,
                    ..params()
                },
            )
            .unwrap();
            assert!(patch.offsets.iter().all(|offset| *offset <= amplitude));
            assert!(patch.offsets.iter().any(|offset| *offset > 0));
        }
    }

    #[test]
    fn a_larger_scale_produces_fewer_adjacent_steps() {
        let small = generate_heightmap(
            64,
            64,
            0,
            0,
            HeightmapParams {
                scale: 2,
                octaves: 1,
                amplitude: 16,
                ..params()
            },
        )
        .unwrap();
        let large = generate_heightmap(
            64,
            64,
            0,
            0,
            HeightmapParams {
                scale: 32,
                octaves: 1,
                amplitude: 16,
                ..params()
            },
        )
        .unwrap();
        let changes = |patch: &HeightmapPatch| {
            patch
                .offsets
                .windows(2)
                .filter(|pair| pair[0] != pair[1])
                .count()
        };
        assert!(changes(&large) < changes(&small));
    }

    #[test]
    fn invalid_sizes_and_parameters_are_rejected() {
        assert!(matches!(
            generate_heightmap(0, 4, 0, 0, params()),
            Err(GenerationError::EmptySize { .. })
        ));
        assert!(matches!(
            generate_heightmap(1025, 1024, 0, 0, params()),
            Err(GenerationError::TooManyColumns { .. })
        ));
        for scale in [0, 65] {
            assert!(matches!(
                generate_heightmap(4, 4, 0, 0, HeightmapParams { scale, ..params() }),
                Err(GenerationError::InvalidScale(_))
            ));
        }
        for octaves in [0, 7] {
            assert!(matches!(
                generate_heightmap(
                    4,
                    4,
                    0,
                    0,
                    HeightmapParams {
                        octaves,
                        ..params()
                    }
                ),
                Err(GenerationError::InvalidOctaves(_))
            ));
        }
        for amplitude in [0, 257] {
            assert!(matches!(
                generate_heightmap(
                    4,
                    4,
                    0,
                    0,
                    HeightmapParams {
                        amplitude,
                        ..params()
                    }
                ),
                Err(GenerationError::InvalidAmplitude(_))
            ));
        }
    }
}
