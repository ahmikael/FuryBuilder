import type { CSSProperties } from "react";

import type { CatalogAtlasTexture, CatalogIconAtlas } from "@/assets/types";
import { blockColor } from "@/renderer/assets";

export interface CatalogIconSprite {
  url: string;
  atlasWidth: number;
  atlasHeight: number;
  texture: CatalogAtlasTexture;
}

/** Every loaded lazy atlas batch merged into one texture-key lookup. */
export type CatalogIconIndex = ReadonlyMap<string, CatalogIconSprite>;

/** Builds the icon lookup once per set of loaded atlas batches. */
export function catalogIconIndex(atlases: readonly CatalogIconAtlas[]): CatalogIconIndex {
  const index = new Map<string, CatalogIconSprite>();
  for (const atlas of atlases) {
    const url = `data:image/png;base64,${atlas.pngBase64}`;
    const atlasWidth = Math.max(1, atlas.width);
    const atlasHeight = Math.max(1, atlas.height);
    for (const texture of atlas.textures) {
      index.set(texture.key, { url, atlasWidth, atlasHeight, texture });
    }
  }
  return index;
}

interface CatalogBlockIconProps {
  name: string;
  textureKey: string | null;
  icons: CatalogIconIndex | null;
  size?: number;
  mode?: "sprite" | "cube";
}

function spriteStyle(sprite: CatalogIconSprite, size: number): CSSProperties {
  const { texture, url, atlasWidth, atlasHeight } = sprite;
  const scaleX = size / Math.max(1, texture.width);
  const scaleY = size / Math.max(1, texture.height);
  return {
    width: size,
    height: size,
    backgroundImage: `url("${url}")`,
    backgroundSize: `${atlasWidth * scaleX}px ${atlasHeight * scaleY}px`,
    backgroundPosition: `${-texture.x * scaleX}px ${-texture.y * scaleY}px`,
    backgroundRepeat: "no-repeat",
    imageRendering: "pixelated",
  };
}

/**
 * A catalog icon. The flat chip is the honest fallback when no atlas texture is available;
 * `cube` is an isometric presentation of the same atlas sprite, not a full model render.
 */
export function CatalogBlockIcon({
  name,
  textureKey,
  icons,
  size = 20,
  mode = "sprite",
}: CatalogBlockIconProps) {
  const sprite = textureKey ? icons?.get(textureKey) : undefined;
  if (!sprite || sprite.texture.missing) {
    if (mode === "cube") {
      const face = Math.max(8, Math.round(size * 0.72));
      const half = face / 2;
      const color = blockColor(name);
      const solidFace = (transform: string, filter: string) => (
        <span className="absolute inset-0" style={{ backgroundColor: color, transform, filter }} />
      );
      return (
        <span
          data-catalog-icon="flat-cube"
          className="relative inline-grid shrink-0 place-items-center"
          style={{ width: size, height: size }}
          aria-hidden="true"
        >
          <span
            className="relative"
            style={{
              width: face,
              height: face,
              transformStyle: "preserve-3d",
              transform: "rotateX(-30deg) rotateY(45deg)",
            }}
          >
            {solidFace(`translateZ(${half}px)`, "brightness(0.9)")}
            {solidFace(`rotateY(90deg) translateZ(${half}px)`, "brightness(0.72)")}
            {solidFace(`rotateX(90deg) translateZ(${half}px)`, "brightness(1.08)")}
          </span>
        </span>
      );
    }
    return (
      <span
        data-catalog-icon="flat"
        className="shrink-0 rounded-sm border"
        style={{ width: size, height: size, backgroundColor: blockColor(name) }}
        aria-hidden="true"
      />
    );
  }

  if (mode === "sprite") {
    return (
      <span
        data-catalog-icon="sprite"
        className="shrink-0 rounded-sm"
        style={spriteStyle(sprite, size)}
        aria-hidden="true"
      />
    );
  }

  const face = Math.max(8, Math.round(size * 0.72));
  const half = face / 2;
  const faceStyle = spriteStyle(sprite, face);
  return (
    <span
      data-catalog-icon="cube"
      className="relative inline-grid shrink-0 place-items-center"
      style={{ width: size, height: size }}
      aria-hidden="true"
    >
      <span
        className="relative"
        style={{
          width: face,
          height: face,
          transformStyle: "preserve-3d",
          transform: "rotateX(-30deg) rotateY(45deg)",
        }}
      >
        <span
          className="absolute inset-0"
          style={{ ...faceStyle, transform: `translateZ(${half}px)`, filter: "brightness(0.9)" }}
        />
        <span
          className="absolute inset-0"
          style={{
            ...faceStyle,
            transform: `rotateY(90deg) translateZ(${half}px)`,
            filter: "brightness(0.72)",
          }}
        />
        <span
          className="absolute inset-0"
          style={{
            ...faceStyle,
            transform: `rotateX(90deg) translateZ(${half}px)`,
            filter: "brightness(1.08)",
          }}
        />
      </span>
    </span>
  );
}
