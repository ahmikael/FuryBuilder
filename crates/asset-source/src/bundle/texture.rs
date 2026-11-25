use std::io::Cursor;

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;
use serde::Serialize;

use crate::bundle::stack::ResourceStack;
use crate::bundle::{normalize_id, BundleError};

const MAX_TEXTURES: usize = 512;
const MAX_TEXTURE_DIMENSION: u32 = 1024;
const MAX_TEXTURE_PIXELS: u32 = 1024 * 1024;
const MAX_ATLAS_DIMENSION: u32 = 4096;
const MAX_ATLAS_PIXELS: u64 = 32 * 1024 * 1024;
const PADDING: u32 = 1;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BundleTexture {
    pub key: String,
    pub path: Option<String>,
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
    pub source_width: u32,
    pub source_height: u32,
    pub missing: bool,
    pub has_alpha: bool,
}

pub struct BuiltAtlas {
    pub width: u32,
    pub height: u32,
    pub png_base64: String,
    pub textures: Vec<BundleTexture>,
    pub warnings: Vec<String>,
}

#[derive(Clone)]
struct DecodedTexture {
    key: String,
    path: Option<String>,
    width: u32,
    height: u32,
    source_width: u32,
    source_height: u32,
    rgba: Vec<u8>,
    missing: bool,
    has_alpha: bool,
}

struct DecodedImage {
    source_width: u32,
    source_height: u32,
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}

pub(crate) fn build_atlas(
    stack: &mut ResourceStack,
    keys: &std::collections::BTreeSet<String>,
) -> Result<BuiltAtlas, BundleError> {
    build_atlas_with_limit(stack, keys, MAX_TEXTURES)
}

pub(crate) fn build_atlas_with_limit(
    stack: &mut ResourceStack,
    keys: &std::collections::BTreeSet<String>,
    max_textures: usize,
) -> Result<BuiltAtlas, BundleError> {
    let mut warnings = Vec::new();
    let mut textures: Vec<DecodedTexture> = Vec::new();
    let mut ordered: Vec<String> = keys.iter().cloned().collect();
    ordered.push("__missing__".to_owned());
    ordered.sort();
    ordered.dedup();
    if ordered.len() > max_textures {
        return Err(BundleError::LimitExceeded(format!(
            "an atlas references more than {max_textures} textures"
        )));
    }
    let missing_checker = missing_texture("__missing__", None);
    for key in ordered {
        if key == "__missing__" {
            textures.push(missing_checker.clone());
            continue;
        }
        let Some(path) = texture_path(&key) else {
            warnings.push(format!("texture key {key} cannot be mapped to a path"));
            textures.push(missing_texture(&key, None));
            continue;
        };
        let Some(bytes) = stack.read_texture(&path)? else {
            warnings.push(format!(
                "texture {path} is missing; the missing checker is used"
            ));
            textures.push(missing_texture(&key, Some(path)));
            continue;
        };
        match decode_png(&bytes) {
            Ok(image) => {
                let has_alpha = image
                    .rgba
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .any(|pixel| pixel[3] < 255);
                textures.push(DecodedTexture {
                    key,
                    path: Some(path),
                    width: image.width,
                    height: image.height,
                    source_width: image.source_width,
                    source_height: image.source_height,
                    rgba: image.rgba,
                    missing: false,
                    has_alpha,
                })
            }
            Err(error) => {
                warnings.push(format!("texture {path} is not usable: {error}"));
                textures.push(missing_texture(&key, Some(path)));
            }
        }
    }
    let (width, height, placements) = pack_textures(&textures)?;
    let mut atlas = vec![0u8; width as usize * height as usize * 4];
    for (texture, (x, y)) in textures.iter().zip(placements.iter()) {
        blit(&mut atlas, width, height, *x, *y, texture);
    }
    let png = encode_png(width, height, &atlas)?;
    let entries = textures
        .iter()
        .zip(placements.iter())
        .map(|(texture, (x, y))| BundleTexture {
            key: texture.key.clone(),
            path: texture.path.clone(),
            x: x + PADDING,
            y: y + PADDING,
            width: texture.width,
            height: texture.height,
            source_width: texture.source_width,
            source_height: texture.source_height,
            missing: texture.missing,
            has_alpha: texture.has_alpha,
        })
        .collect();
    Ok(BuiltAtlas {
        width,
        height,
        png_base64: BASE64.encode(png),
        textures: entries,
        warnings,
    })
}

fn texture_path(key: &str) -> Option<String> {
    if key == "__missing__" {
        return None;
    }
    let normalized = normalize_id(key)?;
    let (namespace, path) = normalized.split_once(':')?;
    Some(format!("assets/{namespace}/textures/{path}.png"))
}

fn missing_texture(key: &str, path: Option<String>) -> DecodedTexture {
    let width = 16u32;
    let height = 16u32;
    let mut rgba = vec![0u8; (width * height * 4) as usize];
    for y in 0..height {
        for x in 0..width {
            let magenta = ((x / 4) + (y / 4)).is_multiple_of(2);
            let offset = ((y * width + x) * 4) as usize;
            if magenta {
                rgba[offset] = 255;
                rgba[offset + 1] = 0;
                rgba[offset + 2] = 255;
                rgba[offset + 3] = 255;
            } else {
                rgba[offset] = 24;
                rgba[offset + 1] = 24;
                rgba[offset + 2] = 24;
                rgba[offset + 3] = 255;
            }
        }
    }
    DecodedTexture {
        key: key.to_owned(),
        path,
        width,
        height,
        source_width: width,
        source_height: height,
        rgba,
        missing: true,
        has_alpha: false,
    }
}

type ShelfPacking = (u32, u32, Vec<(u32, u32)>);

fn pack_textures(textures: &[DecodedTexture]) -> Result<ShelfPacking, BundleError> {
    let mut order: Vec<usize> = (0..textures.len()).collect();
    order.sort_by(|left, right| {
        let a = &textures[*left];
        let b = &textures[*right];
        b.height
            .cmp(&a.height)
            .then_with(|| b.width.cmp(&a.width))
            .then_with(|| a.key.cmp(&b.key))
    });
    for width in [256u32, 512, 1024, 2048, 4096] {
        if let Some((height, mut placements)) = shelf_pack(textures, &order, width) {
            let height = height.next_power_of_two().max(16);
            if width as u64 * height as u64 > MAX_ATLAS_PIXELS {
                continue;
            }
            let mut by_index = vec![(0u32, 0u32); textures.len()];
            for (position, index) in order.iter().enumerate() {
                by_index[*index] = placements[position];
            }
            placements.clear();
            return Ok((width, height, by_index));
        }
    }
    Err(BundleError::LimitExceeded(format!(
        "textures do not fit into a {MAX_ATLAS_DIMENSION}x{MAX_ATLAS_DIMENSION} atlas"
    )))
}

fn shelf_pack(
    textures: &[DecodedTexture],
    order: &[usize],
    width: u32,
) -> Option<(u32, Vec<(u32, u32)>)> {
    let mut placements = vec![(0u32, 0u32); textures.len()];
    let mut x = 0u32;
    let mut y = 0u32;
    let mut shelf = 0u32;
    for index in order {
        let texture = &textures[*index];
        let item_width = texture.width + PADDING * 2;
        let item_height = texture.height + PADDING * 2;
        if item_width > width {
            return None;
        }
        if x + item_width > width {
            x = 0;
            y = y.checked_add(shelf)?;
            shelf = 0;
        }
        placements[*index] = (x, y);
        x += item_width;
        shelf = shelf.max(item_height);
    }
    let height = y.checked_add(shelf)?;
    if height > MAX_ATLAS_DIMENSION {
        return None;
    }
    Some((height, placements))
}

fn blit(
    atlas: &mut [u8],
    atlas_width: u32,
    atlas_height: u32,
    x: u32,
    y: u32,
    texture: &DecodedTexture,
) {
    for dy in -1i32..=texture.height as i32 {
        for dx in -1i32..=texture.width as i32 {
            let source_x = dx.clamp(0, texture.width as i32 - 1) as u32;
            let source_y = dy.clamp(0, texture.height as i32 - 1) as u32;
            let target_x = x as i32 + PADDING as i32 + dx;
            let target_y = y as i32 + PADDING as i32 + dy;
            if target_x < 0
                || target_y < 0
                || target_x >= atlas_width as i32
                || target_y >= atlas_height as i32
            {
                continue;
            }
            let source = ((source_y * texture.width + source_x) * 4) as usize;
            let target = ((target_y as u32 * atlas_width + target_x as u32) * 4) as usize;
            atlas[target..target + 4].copy_from_slice(&texture.rgba[source..source + 4]);
        }
    }
}

fn decode_png(bytes: &[u8]) -> Result<DecodedImage, String> {
    let mut decoder = png::Decoder::new(Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().map_err(|error| error.to_string())?;
    let info = reader.info();
    let source_width = info.width;
    let source_height = info.height;
    if source_width == 0
        || source_height == 0
        || source_width > MAX_TEXTURE_DIMENSION
        || source_height > MAX_TEXTURE_DIMENSION
    {
        return Err(format!(
            "dimensions {source_width}x{source_height} are outside 1..={MAX_TEXTURE_DIMENSION}"
        ));
    }
    if source_width.saturating_mul(source_height) > MAX_TEXTURE_PIXELS {
        return Err("texture has too many pixels".to_owned());
    }
    let mut buffer = vec![0u8; reader.output_buffer_size()];
    let frame = reader
        .next_frame(&mut buffer)
        .map_err(|error| error.to_string())?;
    let data = &buffer[..frame.buffer_size()];
    let stride = frame.line_size;
    let mut rgba = vec![0u8; source_width as usize * source_height as usize * 4];
    for row in 0..source_height as usize {
        let start = row * stride;
        for column in 0..source_width as usize {
            let target = (row * source_width as usize + column) * 4;
            match frame.color_type {
                png::ColorType::Rgba => {
                    let source = start + column * 4;
                    rgba[target..target + 4].copy_from_slice(&data[source..source + 4]);
                }
                png::ColorType::Rgb => {
                    let source = start + column * 3;
                    rgba[target] = data[source];
                    rgba[target + 1] = data[source + 1];
                    rgba[target + 2] = data[source + 2];
                    rgba[target + 3] = 255;
                }
                png::ColorType::Grayscale => {
                    let value = data[start + column];
                    rgba[target] = value;
                    rgba[target + 1] = value;
                    rgba[target + 2] = value;
                    rgba[target + 3] = 255;
                }
                png::ColorType::GrayscaleAlpha => {
                    let source = start + column * 2;
                    let value = data[source];
                    rgba[target] = value;
                    rgba[target + 1] = value;
                    rgba[target + 2] = value;
                    rgba[target + 3] = data[source + 1];
                }
                png::ColorType::Indexed => {
                    return Err("indexed PNG was not expanded".to_owned());
                }
            }
        }
    }
    // Vanilla animated block textures are square frames stacked vertically (for example
    // 16x512 water). FuryBuilder samples the first frame: the stack would otherwise stretch
    // the whole animation into one UV rectangle and inflate the atlas. Animation timing is
    // not played back.
    let height = if source_height > source_width && source_height.is_multiple_of(source_width) {
        source_width
    } else {
        source_height
    };
    if height != source_height {
        let bytes = source_width as usize * height as usize * 4;
        rgba.truncate(bytes);
    }
    Ok(DecodedImage {
        source_width,
        source_height,
        width: source_width,
        height,
        rgba,
    })
}

fn encode_png(width: u32, height: u32, rgba: &[u8]) -> Result<Vec<u8>, BundleError> {
    let mut bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut bytes, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder
            .write_header()
            .map_err(|error| BundleError::Io(format!("cannot encode atlas: {error}")))?;
        writer
            .write_image_data(rgba)
            .map_err(|error| BundleError::Io(format!("cannot encode atlas: {error}")))?;
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid_texture(key: &str, width: u32, height: u32, color: [u8; 4]) -> DecodedTexture {
        DecodedTexture {
            key: key.to_owned(),
            path: None,
            width,
            height,
            source_width: width,
            source_height: height,
            rgba: color
                .iter()
                .copied()
                .cycle()
                .take((width * height * 4) as usize)
                .collect(),
            missing: false,
            has_alpha: false,
        }
    }

    #[test]
    fn deterministic_shelf_packing_places_all_textures() {
        let textures = vec![
            solid_texture("a", 16, 16, [255, 0, 0, 255]),
            solid_texture("b", 32, 16, [0, 255, 0, 255]),
            solid_texture("c", 16, 32, [0, 0, 255, 255]),
        ];
        let (width, height, placements) = pack_textures(&textures).unwrap();
        assert!(width >= 256);
        assert!(height >= 16);
        for (index, (x, y)) in placements.iter().enumerate() {
            let texture = &textures[index];
            assert!(*x + texture.width + PADDING * 2 <= width);
            assert!(*y + texture.height + PADDING * 2 <= height);
        }
        let again = pack_textures(&textures).unwrap();
        assert_eq!(placements, again.2);
        assert_eq!((width, height), (again.0, again.1));
    }

    #[test]
    fn missing_checker_is_opaque_and_tileable() {
        let missing = missing_texture("__missing__", None);
        assert_eq!((missing.width, missing.height), (16, 16));
        assert!(missing.missing);
        assert!(missing
            .rgba
            .as_chunks::<4>()
            .0
            .iter()
            .all(|pixel| pixel[3] == 255));
        assert!(missing
            .rgba
            .as_chunks::<4>()
            .0
            .iter()
            .any(|pixel| pixel[0] == 255));
    }

    #[test]
    fn animated_texture_uses_only_its_first_frame() {
        let mut png = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut png, 16, 32);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().expect("png header");
            let mut pixels = vec![0u8; 16 * 32 * 4];
            for (index, pixel) in pixels.as_chunks_mut::<4>().0.iter_mut().enumerate() {
                let row = index / 16;
                pixel.copy_from_slice(if row < 16 {
                    &[255, 0, 0, 255]
                } else {
                    &[0, 255, 0, 255]
                });
            }
            writer.write_image_data(&pixels).expect("png data");
        }
        let decoded = decode_png(&png).expect("decode animated png");
        assert_eq!((decoded.source_width, decoded.source_height), (16, 32));
        assert_eq!((decoded.width, decoded.height), (16, 16));
        assert_eq!(decoded.rgba.len(), 16 * 16 * 4);
        assert!(decoded
            .rgba
            .as_chunks::<4>()
            .0
            .iter()
            .all(|pixel| *pixel == [255, 0, 0, 255]));
    }

    #[test]
    fn texture_paths_are_namespaced() {
        assert_eq!(
            texture_path("minecraft:block/stone").as_deref(),
            Some("assets/minecraft/textures/block/stone.png")
        );
        assert_eq!(
            texture_path("block/stone").as_deref(),
            Some("assets/minecraft/textures/block/stone.png")
        );
        assert!(texture_path("__missing__").is_none());
    }
}
