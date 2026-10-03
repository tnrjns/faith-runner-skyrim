# Faith Runner for Skyrim

An SKSE plugin that puts Mirror's Edge's movement on Skyrim's player:
- **The movement** is the same `faith_move` the app runs: wallruns, wallclimbs, vaults, ledge grabs, slides, rolls, 180s, dodges, on Skyrim's own collision.
- **The animation** is the same as well. With a Mirror's Edge install, Faith's own animations (`faith_anim`) play on the player's skeletons: the first-person arms and the third-person body.
- **The camera** in first person is hers: the camera bone, bob, rolls and the swan neck.

Nothing from either game ships with it. Mirror's Edge's animations are read from your install when Skyrim starts.

## Using it

Requirements:
- Skyrim Special Edition, **Anniversary Edition runtime** (built and tested against 1.7.104). Not 1.5.97, not VR.
- [SKSE64](https://skse.silverlock.org/) for your game version.
- [Address Library for SKSE Plugins](https://www.nexusmods.com/skyrimspecialedition/mods/32444) ("All in one (Anniversary Edition)").
- Mirror's Edge installed, for the animations. Without it Faith still moves, but nothing is animated. If it isn't in a usual folder, set `sMirrorsEdgeDir` in `FaithSkyrim.ini`.

Install: the build drops `FaithSkyrim.dll` and `FaithSkyrim.ini` into an MO2 mod called **Faith Runner** (`SKSE\Plugins`). Enable it in Mod Organizer and start Skyrim through SKSE.

In game, **F8** switches Faith on and off (`iToggleKey`). While she's on:

| | |
|---|---|
| WASD | move |
| Mouse | look |
| Space | jump, vault, wall moves |
| Shift / C / Ctrl | crouch, slide, roll (tap before landing) |
| A or D + Space | dodge |
| Q | 180 turn (on a wall: climb, Q, Space to kick off) |
| Left mouse / F | attack, barge |
| F7 | first person shows: Faith's own body → Skyrim's whole body → Skyrim's arms |
| G | play one of Faith's idles (she also plays them by herself after 30-40 s standing still, as in Mirror's Edge) |

Everything else stays Skyrim's: E to activate, menus, favourites, shouts, waiting. In menus, dialogue, furniture, on horseback and in kill moves, Skyrim has the player until it lets go.

`Documents\My Games\Skyrim Special Edition\SKSE\FaithSkyrim.log` says what it found:
- the Mirror's Edge animations;
- how many bones of each skeleton Faith drives;
- how much collision it reads;
- the camera's axes.

## How it works

`src/` is the plugin (C++, CommonLibSSE-NG). The Rust crates do the movement and animation, through `crates/faith_ffi` (`include/faith.h`).

**Per frame** (`Faith.cpp`), all on the main thread:
- **After `PlayerCharacter::Update`:**
  1. Read the controls (`Input.cpp`). Skyrim's own player controls lose Faith's keys and the mouse look, and keep the rest.
  2. Read the collision again if it's due.
  3. Step Faith.
  4. Move the player's reference and Havok capsule to her feet, with no momentum or fall damage of Skyrim's own.
  5. Point the player's look where she looks, so Skyrim's AI, aiming and activation follow it.
- **After the animation update and after `PlayerCamera::Update`:**
  - Faith's pose goes on the skeletons.
  - In first person, her camera replaces Skyrim's.
  - The camera root's axis convention is learnt in the first couple of seconds, by watching Skyrim build it from the look angles (SkyCraft's method). Until then only its position is set.

**Collision** (`Collision.cpp`):
- **What it reads:** every Havok shape of the world around the player, out to 24 m by default. That's triangle meshes, MOPP trees, boxes, capsules, convex hulls and transforms, from the static, animated-static, terrain, ground, trees, props, glass and invisible-wall layers.
- **What it becomes:** the shapes turn into triangles, which become `faith_move`'s `MeshWorld`.
- **When it's read:** twice a second, and whenever you've moved 6 m.
- **What's left out:** loose clutter, characters and Skyrim's invisible stair ramps.

**Faith's own body** (`Viewmodel.cpp`, `faith_ffi/src/body.rs`):
- **What it is:** in first person you see Mirror's Edge's own first-person body, as in the app: arms and torso (`SK_UpperBody`) and legs (`SK_LowerBody`). They're skinned every frame by the app's own code, forearm twist morphs included.
- **How it's drawn:** straight into Skyrim's HDR scene after `Main::RenderWorld`, before tone mapping, bloom and grading.
  - **Lighting:** Skyrim's sun or interior light, directional ambient and nearest point lights, gathered the way SkyCraft does.
  - **Shadows:** Skyrim's own sun shadows fall on her: its cascades are copied the moment the sun's shadow pass ends (`BSShadowDirectionalLight::Render`, SkyCraft's method) and sampled for the sunlight.
  - **Arms and torso:** like Mirror's Edge's foreground body, they have their own depth buffer and field of view (Model1pFOV 100), blending to Skyrim's FOV as you look down, so they never clip into walls.
  - **Legs:** in the world, as the app draws them. They use Skyrim's own camera matrix and depth buffer, so walls hide them, and the arms draw over them.
- **Skyrim's arms:** hidden meanwhile, but still posed like Faith's underneath, with their hands pinned exactly onto hers.
  - **What they hold stays:** weapons, shields and spells, so they sit in Faith's grip.
  - **Matching FOV:** Skyrim's first-person field of view follows her arms' each frame, so they line up on screen.
  - F7 (`iViewmodelKey`) shows Skyrim's arms instead.

**Skyrim's body** (the F7 view after Faith's): Skyrim only draws the player's body in third person, so this view switches to Skyrim's third-person camera and puts it at Faith's eyes.
- **No fading:** Skyrim fades the player out when its third-person camera comes close; the fade is held at full.
- **Head hidden:** the face and hair, and helmets, hoods and circlets.
- **Posed like Faith:** the shoulders sit exactly where Faith's do against her camera, and the hips follow hers, scaled to the body's legs.

**Community Shaders** lights the world deferred and paints over anything drawn into it. With it loaded (or `iDrawStage=2`), Faith's body and the speed blur are drawn late instead: just before the HUD, after all post-processing, tone-mapped in her own shader.

**Speed blur** (`bSpeedBlur`): Mirror's Edge's own sprint blur, `TdMotionBlurShader.usf` as shipped, in first person, over the scene before Skyrim's tone mapping.
- **How strong:** `TdMotionBlurPostProcess`'s render proxy (`0x12d42a0`) measures the camera's speed frame to frame (each axis held to ±720 uu/s, smoothed), ramps it from 400 uu/s to 720, and scales it by `TdMotionBlurAmount` (0.5) and by how much you move along your view (`faith_move::SpeedBlur`).
- **Its direction:** the shader keeps only one component of its blur direction and steps both u and v by it, so the blur runs diagonally. Kept, because that's how it looks in the game.

**Upscalers** (Community Shaders' DLSS, FSR, XeSS): they draw the scene into part of the target (Skyrim's dynamic resolution) and scale it up after, so Faith's body and the blur go in that same part.

**Staying on the ground:**
- **Precise far out:** Skyrim's world spans kilometres, where a float in metres is only good to a quarter millimetre. So faith_move works around an origin that's re-centred on the player every 200 m (`faith_ffi`).
- **Can't sink through:** a surface she's sunk into by a hair holds her instead of being ignored (`MeshWorld`'s skin).
- **Safety net:** if she ever drops 40 m below where she last stood, she's put back there.
- **Near plane:** Skyrim's is lowered to 3 units while Faith is on (`fNearDistance`), so walls right in front of her camera aren't cut away.

**Sounds** (`faith_ffi/src/audio.rs`, `faith_anim::sound`):
- **What plays:** the app's own sound logic, shared, through the default sound device. That covers the footsteps, handsteps, cloth and breath cues baked into the animations, plus landings, rolls, slides, strains and the rush of wind at speed.
- **Where they come from:** your Mirror's Edge install's sound packages.
- **Surface:** steps are always concrete, since Skyrim's ground doesn't say what it's made of the way Mirror's Edge's does.
- **When they pause:** in menus and while Skyrim has the player.
- **Volume:** `fSoundVolume`.

**Skeletons** (`Body.cpp`, `faith_anim::retarget`):
- **Binding:** each skeleton is bound once from its rest pose, read from the skeleton NIF the game loads.
  - The body uses the race's skeleton.
  - The arms use `_1stPerson`.
  - If neither can be read, the live pose stands in.
- **Rotations:**
  - Faith's bones drive Skyrim's by name: spine, neck, head, clavicles, arms, forearm twists, hands, all fifteen finger joints per hand, legs and feet.
  - Each bone carries over through a fixed offset, found by turning Faith's rest pose until each bone points along its Skyrim counterpart.
- **Positions:** Skyrim's skeleton keeps its own lengths, with two exceptions.
  - The hips follow Faith's, scaled to the skeleton's legs, so crouches, dips and rolls carry over.
  - In first person the shoulders sit exactly where Faith's do against her camera.
- **Output:** every bone of the skeleton gets a local transform each frame, so none of Skyrim's own animation shows through.

**Credit:** the collision reading and the hooks follow [SkyCraft](https://github.com/chasmlol/SkyCraft) (MIT License, Copyright (c) 2026 chasmlol), which drives Skyrim's player the same way on 1.7.104.

**Licence:** CommonLibSSE-NG is GPL-3.0, so the built plugin is too.

## Parkour cities (Riften, Solitude)

1. **Survey:** in each city, stand near the middle and press **F10** (`iSurveyKey`). Everything loaded around you is saved to `FaithSurvey_<worldspace>.bin` in the SKSE log folder.
2. **Build:** run `tools/parkour.sh`. It writes the MO2 mod **Faith Runner Parkour**:
   - **`SKSE/Plugins/FaithParkour/<worldspace>.bin`:** Faith-only collision fixes, which the plugin applies on top of Skyrim's collision in that worldspace. These are thin invisible fillers over the cracks between neighbouring rooftops that she'd trip or snag in.
   - **`FaithParkour.esp`:** visible connectors across the rooftop gaps that are just out of her reach, up to 12 m across and 1 m up or down. Riften gets its own dock planks (`RTDockRamp01`, end to end, scaled up to 1.5×). Solitude gets scaffold bridges (`StockadeScaffoldBridge01`–`03`). They're Skyrim's own models with their own collision, so she uses them like any other ledge.

How the gaps are found (`crates/parkour_tool`):
- **Rooftops:** every walkable surface standing 2.5 m or more above the ground under it, in 0.5 m cells, joined into rooftops.
- **Gaps:** the closest edge points between rooftops, with nothing in the way at chest height.
- **What she can reach:** decided by Faith's own movement. Every distance and height, every 0.25 m, is run in `faith_move`: sprint to the edge, jump, grab and pull up. She makes 8.25 m flat and about 10 m dropping 3 m.
- **Where the bridges go:** shortest first, only joining rooftops she can't already get between, at most 30 per city.

## Building

You need:
- Visual Studio 2026 Build Tools (C++ and CMake);
- Rust (stable, MSVC);
- git.

Then run, in this folder:

```
git clone https://github.com/alandtse/CommonLibVR.git extern/CommonLibSSE-NG
git -C extern/CommonLibSSE-NG checkout d61bca4de789428aa7d98a770b1323ddf1bb855c
git -C extern/CommonLibSSE-NG submodule update --init --recursive
git clone https://github.com/microsoft/vcpkg.git .tools/vcpkg
.tools\vcpkg\bootstrap-vcpkg.bat -disableMetrics
cmake --preset default
cmake --build --preset release
```

The build runs `cargo build -p faith_ffi --release` itself and links the result in. `FAITH_DEPLOY_DIR` (in `CMakePresets.json`) is the MO2 mod folder it copies into.

## Not there yet

- **Moving objects:** they collide as they were when last read.
- **Fixtures:** Mirror's Edge's ziplines, swing poles and balance beams need markers in the world, and Skyrim has none.
- **Combat:** Faith's attacks and barges animate but don't hit anything.
- **Faith's body:** only the sun shadows it (no torch or spell shadows). Held weapons are drawn under her arms, not wrapped by her fingers.
- **Third-person camera:** it's Skyrim's own, following her.
