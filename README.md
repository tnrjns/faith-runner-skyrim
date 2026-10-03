# Faith Runner for Skyrim

An SKSE plugin that puts Mirror's Edge's movement, animation and camera on Skyrim's player:
- **The movement** is [Faith Runner](https://github.com/tnrjns/faith-runner)'s `faith_move`, running on Skyrim's own collision. That covers wallruns, wallclimbs, vaults, ledge grabs, slides, rolls, 180s, dodges and springboards.
- **The animation** is Faith's own, read from your Mirror's Edge install, on the player's first-person arms and third-person body.
- **The camera** in first person is hers: the camera bone, bob, rolls, the swan neck and Mirror's Edge's speed blur.
- **The training courses** from the app are playable too, built in the sky above you.

Nothing from either game ships with it. Mirror's Edge's animations and sounds are read from your install when Skyrim starts.

`crates/` holds a copy of Faith Runner's movement and animation crates (`faith_move`, `faith_anim`, `me_assets`), plus the parts that exist only for Skyrim:
- `faith_ffi`, the C interface the plugin links;
- `faith_anim`'s skeleton retargeting;
- `parkour_tool`.

---

## Prerequisites

### To play

| | |
|---|---|
| **Skyrim Special Edition, Anniversary Edition runtime** | Built and tested on **1.7.104**. Not 1.5.97, not VR. |
| **[SKSE64](https://skse.silverlock.org/)** | The build for your game version. |
| **[Address Library for SKSE Plugins](https://www.nexusmods.com/skyrimspecialedition/mods/32444)** | The "All in one (Anniversary Edition)" file. |
| **Mirror's Edge (PC)** | For Faith's animations, body and sounds. Without it she still moves, but nothing is animated. If it isn't in a usual folder (Steam, EA, Origin, `C:\Games\Mirror's Edge`), set `sMirrorsEdgeDir` in the ini. |
| **[Mod Organizer 2](https://github.com/ModOrganizer2/modorganizer)** | Recommended. The build copies itself straight into an MO2 mod. |

### To tweak it in game (optional, recommended)

| | |
|---|---|
| **[SKSE Menu Framework](https://www.nexusmods.com/skyrimspecialedition/mods/120352)** (3.x) | Adds the **Mod Control Panel**, with a Faith Runner page for every setting, the training courses and an on-screen course timer. Without it, everything still works from `FaithSkyrim.ini`. |
| **[ImGui Icons](https://www.nexusmods.com/skyrimspecialedition/mods/114790)** | Required by SKSE Menu Framework 3.15 and later. |

### Works with

- **[Community Shaders](https://www.nexusmods.com/skyrimspecialedition/mods/86492):** detected automatically. Faith's body and the speed blur are then drawn after its lighting (see `iDrawStage`).
- **Upscalers** (DLSS, FSR, XeSS through Community Shaders): Faith is drawn into the same part of the screen they scale up.

---

## Installing

Put `FaithSkyrim.dll` and `FaithSkyrim.ini` into `Data\SKSE\Plugins` (for MO2, a mod with `SKSE\Plugins\` inside). Then start Skyrim through SKSE.

A build from source does this for you (see [Building](#building)).

There's **no ESP**. If an old save complains that `FaithParkour.esp` is missing (an early version had one), choose **Continue** and save again.

---

## Controls

**F8** switches Faith on and off. While she's on:

| Key | |
|---|---|
| WASD | Move |
| Mouse | Look |
| Space | Jump, vault, wall moves, springboard |
| Shift / C / Ctrl | Crouch, slide, roll (tap before landing) |
| A or D + Space | Dodge |
| Q | 180 turn (on a wall: climb, Q, Space to kick off) |
| Left mouse / F | Attack, barge, kick doors |
| **Caps Lock** | Walk (toggle), like Skyrim's always-run |
| F7 | First-person view: Faith's own body → Skyrim's whole body → Skyrim's arms |
| G | Play one of Faith's idles. She also plays them by herself after 30-40 s standing still. |
| R | On a training course: back to the last checkpoint |
| F10 | Survey the collision around you, for the parkour tool |
| F1 | SKSE Menu Framework's Mod Control Panel (its own key) |

Everything else stays Skyrim's: E to activate, menus, favourites, shouts and waiting. In menus, dialogue, furniture, on horseback and in kill moves, Skyrim keeps the player until it lets go.

You can rebind every key: on the menu's Keys page, or in the ini as a [DirectInput scan code](https://www.creationkit.com/index.php?title=Input_Script#DXScanCodes).

---

## Tweaking it

### In game: SKSE Menu Framework

Open the **Mod Control Panel** (F1 by default) and go to **Faith Runner**. Changes apply straight away and are saved to `FaithSkyrim.ini` when you close the panel. Things that act on the game (switching Faith on, starting a course, playing an idle) happen as the panel closes, because it pauses the game.

| Page | What's there |
|---|---|
| **General** | Switch Faith on or off. First-person view. Speed blur. Third-person body animation. Play an idle. Mouse sensitivity, sound volume, walking speed, start on with a save. How far the whole-body view's camera sits ahead of the eyes. |
| **Course** | Pick a training map and start or leave it, the run's times, and buttons to jump to any checkpoint. |
| **Keys** | Every key: on/off, view, idle, walk, course respawn, survey. |
| **Advanced** | How Faith's body is drawn (`iDrawStage`), the near clip distances, how much of Skyrim's collision she reads and how often, and where Mirror's Edge was found. |

### In the file: `SKSE\Plugins\FaithSkyrim.ini`

Each setting is commented in the file itself.

**`[General]`**

| Setting | Default | What it does |
|---|---|---|
| `sMirrorsEdgeDir` | *(empty)* | Your Mirror's Edge folder (the one with `TdGame`). Empty: the usual install folders. Read when the game starts. |
| `iToggleKey` | `0x42` (F8) | Switches Faith on and off. |
| `bStartEnabled` | `0` | Faith on as soon as a save loads. |
| `fMouseSensitivity` | `1.0` | Mouse look; 1 is the same as the Faith Runner app. |
| `bThirdPersonBody` | `1` | Animate the third-person body too. |
| `bFaithViewmodel` | `1` | Start in Faith's own first-person body rather than Skyrim's. |
| `iViewmodelKey` | `0x41` (F7) | Goes round the first-person views. |
| `bSkyrimBody` | `1` | Skyrim's view shows its whole body; 0 shows just its arms. |
| `iDrawStage` | `0` | Faith's body and the speed blur. **0** automatic: late with Community Shaders, into the world without. **1** into the world: Skyrim's lighting, legs hidden behind walls. **2** late, just before the HUD. |
| `bSpeedBlur` | `1` | Mirror's Edge's speed blur when running fast. |
| `iIdleKey` | `0x22` (G) | Plays one of Faith's idles. |
| `iWalkKey` | `0x3A` (Caps Lock) | Toggles walking. |
| `fWalkStick` | `0.3` | Walking speed: the keys count as this much of a gamepad stick. Mirror's Edge only sprints with the stick right forward, so walking never sprints. |
| `fBodyCameraForward` | `5` | Whole-body view: the camera sits this many units ahead of Faith's eye, out of the body's neck and collar. |
| `fBodyCameraForwardDown` | `10` | ...plus this much more when looking straight down, so it stays out of the chest. |
| `fNearDistance` | `3.0` | Skyrim's near clip distance while Faith is on (Skyrim's own is 15), so walls close to her camera aren't cut away. 0 leaves Skyrim's. |
| `fNearDistanceBody` | `10.0` | The same, in the whole-body view. |
| `fSoundVolume` | `0.8` | Faith's sounds: footsteps, breathing, landings, wind. 0 is off. |
| `iSurveyKey` | `0x44` (F10) | Saves the collision around you for the parkour tool. |

**`[Collision]`**

| Setting | Default | What it does |
|---|---|---|
| `fRadius` | `2400` | How much of Skyrim's collision around you Faith moves on, in game units (70 a metre). |
| `fHeight` | `1400` | How far above her it reaches. Below, it reaches 60 m, more while falling fast. |
| `fRefreshSeconds` | `0.5` | How often it's read again. It's also re-read whenever you've moved a quarter of the radius, entered a new cell, or are falling fast. |

**`[Course]`**

| Setting | Default | What it does |
|---|---|---|
| `iRespawnKey` | `0x13` (R) | On a course: back to the last checkpoint. Only taken from Skyrim while you're on one. |
| `fHeight` | `20000` | How far above you a course is built (units). Raise it if a mountain pokes through. |

### Troubleshooting

- **The log:** `Documents\My Games\Skyrim Special Edition\SKSE\FaithSkyrim.log` says what was found: Mirror's Edge, the skeletons, the collision read, the camera's axes and how Faith is drawn.
- **Faith's body doesn't show** (Community Shaders, ENB): try `iDrawStage=2`.
- **The screen flickers black while running:** turn off `bSpeedBlur` and say so in an issue.
- **Clipping in the whole-body view:** raise `fBodyCameraForward` / `fBodyCameraForwardDown`, or `fNearDistanceBody`.
- **She falls through something:** the log line `nothing under Faith at (x y z)` gives the spot.

---

## Training courses

These are the app's maps, playable in Skyrim. Open **Mod Control Panel → Faith Runner → Course**, pick one, and press **Start**.

| Map | |
|---|---|
| **Moves** | Springboard, balance beam, swing pole, zipline, a door to barge, barbed wire, a mattress to drop onto, and the finish. |
| **Rooftops** | The app's rooftop run. |
| **Springboard** | Springboard lanes. |
| **Training** | Every move, in order. |

How a course plays:
- **Where it is:** built `fHeight` units above you. While you're on it, it's Faith's whole world, and Skyrim's collision is set aside.
- **Checkpoints and time trial:** they work as in the app. The time starts when you leave the start area, and the time, best and checkpoint show at the top of the screen.
- **Respawning:** falling off, or a deadly fall, puts you back at the last checkpoint. So does **R**.
- **Leaving:** **Leave the course** puts you back exactly where you started it. A save made on a course loads back there too.

---

## Parkour cities

These are invisible, Faith-only collision fixes over the cracks between neighbouring rooftops she'd trip or snag in. Nothing visible is added to the cities.

1. In a city, stand near the middle and press **F10**. Everything loaded around you is saved to `FaithSurvey_<worldspace>.bin` in the SKSE log folder.
2. Run `skyrim/tools/parkour.sh` (Git Bash). It writes `SKSE/Plugins/FaithParkour/<worldspace>.bin` into the MO2 mod **Faith Runner Parkour**, and the plugin applies those on top of Skyrim's collision in that worldspace.

---

## Building

You need:
- **Visual Studio 2026 Build Tools**, with "Desktop development with C++" and CMake;
- **[Rust](https://rustup.rs/)**, stable, MSVC toolchain;
- **git**;
- Mirror's Edge installed, for the animation and sound tests (optional).

```
git clone --recursive https://github.com/tnrjns/faith-runner-skyrim.git
cd faith-runner-skyrim\skyrim
git -C extern/CommonLibSSE-NG submodule update --init --recursive
git clone https://github.com/microsoft/vcpkg.git .tools/vcpkg
.tools\vcpkg\bootstrap-vcpkg.bat -disableMetrics
cmake --preset default
cmake --build --preset release
```

What the build does:
- **Rust:** it runs `cargo build -p faith_ffi --release` itself and links the result into `FaithSkyrim.dll`.
- **Deploying:** `FAITH_DEPLOY_DIR` in `skyrim/CMakePresets.json` is the MO2 mod it copies the DLL into. It defaults to `%LOCALAPPDATA%\ModOrganizer\Skyrim Special Edition\mods\Faith Runner`, so change it for another MO2 instance. The ini is copied only if there isn't one already, so your settings are kept.
- **CommonLibSSE-NG:** a submodule (alandtse's CommonLibVR at `d61bca4`, the commit SkyCraft builds against for 1.7.104), with VR off.

To run the tests:

```
set ME_INSTALL=C:\Games\Mirror's Edge
cargo test --release -p faith_ffi -p faith_anim -p faith_move
```

---

## How it works

`skyrim/src/` is the plugin (C++, CommonLibSSE-NG). The Rust crates do the movement and animation, through `crates/faith_ffi` (`include/faith.h`).

**Per frame** (`FaithMode.cpp`), all on the main thread:
- **After `PlayerCharacter::Update`:**
  1. Read the controls (`Input.cpp`).
  2. Read the collision again if it's due.
  3. Step Faith.
  4. Move the player's reference and Havok capsule to her feet, with none of Skyrim's own momentum or fall damage.
  5. Point the player's look where she looks.
- **After the animation and camera updates:** her pose goes on the skeletons, and in first person her camera replaces Skyrim's.

**Collision** (`Collision.cpp`):
- **What it reads:** every Havok shape around the player: triangle meshes, MOPP trees, boxes, capsules, convex hulls. It covers the static, terrain, ground, trees, props, glass, collision-box, stair and invisible-wall layers, plus clutter big enough to stand on.
- **What it becomes:** triangles, which become `faith_move`'s `MeshWorld`.
- **Staying precise:** `faith_move` works around an origin re-centred on the player every 200 m, since Skyrim's world spans kilometres.
- **Not sinking:** a surface she's sunk into by a hair holds her.
- **Cells still loading:** when there's nothing under her yet, she's held for a moment while it loads.

**Faith's body** (`Viewmodel.cpp`, `faith_ffi/src/body.rs`):
- **What it is:** Mirror's Edge's own first-person arms, torso and legs, skinned every frame by the app's code.
- **How it's lit:** by Skyrim's sun, ambient and nearest lights, with Skyrim's sun shadows.
- **Arms and torso:** their own depth and field of view, so they never clip into walls.
- **Legs:** in the world, so walls hide them.
- **Skyrim's arms:** hidden but posed underneath, hands pinned onto hers, so what they hold sits in her grip.

**Skyrim's whole body** (F7):
- **The camera:** Skyrim's third-person camera, put just ahead of Faith's eyes.
- **Hidden:** the head and head gear.
- **Posed like Faith:** the shoulders sit where hers do against the camera, and the hips follow hers.

**Speed blur:** Mirror's Edge's own `TdMotionBlurShader`, at the strength its `TdMotionBlurPostProcess` works out from the camera's speed.

**Skeletons** (`Body.cpp`, `faith_anim::retarget`): Faith's bones drive Skyrim's by name (spine, neck, head, arms, hands, fingers, legs) through fixed offsets found from the two rest poses. Skyrim's skeleton keeps its own bone lengths.

**Courses** (`faith_ffi/src/course.rs`): the app's `Level` boxes and fixtures become Faith's world, placed at a point in Skyrim. The plugin draws them with Skyrim's camera, into Skyrim's depth.

## Not there yet

- **Moving objects:** they collide as they were when last read.
- **Fixtures in the world:** ziplines, swing poles and balance beams need markers, and Skyrim has none. They exist on the training courses.
- **Combat:** Faith's attacks and barges animate but don't hit anything.

## Credits and licences

- **Movement and animation:** [Faith Runner](https://github.com/tnrjns/faith-runner).
- **[SkyCraft](https://github.com/chasmlol/SkyCraft)** (MIT, Copyright (c) 2026 chasmlol): the collision reading, the hooks and the sun shadow capture follow it.
- **[CommonLibVR / CommonLibSSE-NG](https://github.com/alandtse/CommonLibVR)** (GPL-3.0): a submodule. Because the plugin links it, the built plugin is GPL-3.0.
- **[SKSE Menu Framework](https://github.com/QTR-Modding/SKSE-Menu-Framework-3-Example):** its header, `skyrim/extern/SKSEMenuFramework` (MIT, Copyright (c) 2024 Thiago Kaique).
- **Mirror's Edge** © Electronic Arts / DICE and **Skyrim** © Bethesda. Neither game's files are included; this reads your own install.
