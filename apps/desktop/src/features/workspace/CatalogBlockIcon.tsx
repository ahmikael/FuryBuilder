import type { CSSProperties } from "react";

import type { CatalogAtlasTexture, CatalogIconAtlas } from "@/assets/types";
import { blockColor } from "@/renderer/assets";

export interface CatalogIconAtlasImage {
  url: string;
  width: number;
  height: number;
  textures: ReadonlyMap<string, CatalogAtlasTexture>;
}

/** Precomputes the atlas image and texture index once per catalog load. */
export function catalogIconAtlasImage(atlas: CatalogIconAtlas): CatalogIconAtlasImage {
  return {
    url: `data:image/png;base64,${atlas.pngBase64}`,
    width: Math.max(1, atlas.width),
    height: Math.max(1, atlas.height),
    textures: new Map(atlas.textures.map((texture) => [texture.key, texture])),
  };
}

interface CatalogBlockIconProps {
  name: string;
  textureKey: string | null;
  atlas: CatalogIconAtlasImage | null;
  size?: number;
  mode?: "sprite" | "cube";
}

function spriteStyle(
  atlas: CatalogIconAtlasImage,
  texture: CatalogAtlasTexture,
  size: number,
): CSSProperties {
  const scaleX = size / Math.max(1, texture.width);
  const scaleY = size / Math.max(1, texture.height);
  return {
    width: size,
    height: size,
    backgroundImage: `url("${atlas.url}")`,
    backgroundSize: `${atlas.width * scaleX}px ${atlas.height * scaleY}px`,
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
  atlas,
  size = 20,
  mode = "sprite",
}: CatalogBlockIconProps) {
  const texture = textureKey ? atlas?.textures.get(textureKey) : undefined;
  if (!atlas || !texture || texture.missing) {
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
        style={spriteStyle(atlas, texture, size)}
        aria-hidden="true"
      />
    );
  }

  const face = Math.max(8, Math.round(size * 0.72));
  const half = face / 2;
  const faceStyle = spriteStyle(atlas, texture, face);
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
