# Demo project and screenshot

[`../../examples/furybuilder-demo.furybuilder`](../../examples/furybuilder-demo.furybuilder) is
a real, self-contained FuryBuilder project that opens without Minecraft or Java. It contains
only a FuryBuilder manifest, a deterministic block-state snapshot and an empty source
manifest. No Mojang JAR, texture or asset file is copied into the project or this folder.

The scene is seeded by the built-in `load_demo_world` command: a 3x3 chunk plateau with grass
over dirt and stone, a small pond, a 9x9 oak house on a stone-brick floor, glass windows, a
stair-bordered dark-oak roof, torches, sea-lantern lights and three oak trees. It is
deterministic demo content, not a Minecraft world and not evidence of vanilla worldgen
parity.

![FuryBuilder editor with the demo project open](screenshots/editor.png)

_The real desktop window at 1800x1125, rendered with a locally installed Minecraft 1.21.4
client JAR and asset index._

## Open the demo

1. Install FuryBuilder from the Windows beta package (or run the debug desktop build).
2. Copy the whole `furybuilder-demo.furybuilder` folder into
   `%APPDATA%\com.furybuilder.desktop\projects\`.
3. Open **FuryBuilder Demo** in the Project manager.

The project is read-only in spirit: use it to explore the viewport, observation maps and MCP
read tools. Make a copy before editing if you want to keep the original scene.
