# Hollow Toll (Hollow Knight 3D)

A complete 2.5D Metroidvania in the spirit of a "Hollow Knight 3": tight
combat-platforming on a 2D gameplay plane, with true 3D lighting, depth and parallax.
Sixteen interconnected rooms in four areas, two movement abilities, a mid-boss and a
final boss, aimed at about an hour for a first playthrough (my estimate; nobody has
timed it). Built in Rust with [Bevy](https://bevy.org) 0.18.

*Nym, who cannot hear, climbs the bell-city of Vael. The Bellwarden's toll hollows
all who listen.* Everything here (names, creatures, areas, sounds, art) is original;
it borrows the genre's mechanics and mood, nothing else.

> **Honest status:** the game is complete and verified as far as a machine can verify
> it (see [Verification](#verification)). The first playtest said the knight looked like
> a box, there was no visible sword, the opening was all parkour and the graphics were
> poor; this version answers that (see [Look and feel](#look-and-feel)), but it was
> built without a GPU, so how it looks in motion and how fast it runs on your machine
> are still yours to judge. `docs/DESIGN.md` has a playtest checklist and the ten numbers
> to turn first.

## Play it

1. Install Rust (https://rustup.rs). On **Windows** also install the
   *Visual Studio Build Tools* with the **"Desktop development with C++"** workload.
   On Linux you need the usual Bevy libraries (`libasound2-dev libudev-dev
   libwayland-dev libxkbcommon-dev pkg-config`).
2. From this folder run:

   ```
   cargo play
   ```

   The first build takes several minutes (it compiles the whole engine); after that it
   starts quickly. Always use this optimised build to judge feel.

The game saves at benches (and when you learn something or beat a boss) to
`saves/slot1.ron`; **Continue** on the title screen picks it up. Delete that file (or
choose **New Game**) to start over.

## Controls

The default is a **WASD layout**: the left hand moves and aims, the right hand fights.
The keys from the classic layout (arrows, Space, X, Shift) work as second keys, so
nothing is lost. **Options -> Key layout** switches to the **Classic** layout (arrows
move, Z jump, X attack, C dash).

| Action | Keyboard (WASD layout) | Gamepad |
|---|---|---|
| Move | A / D (or Left / Right) | D-pad / left stick |
| Jump (hold = higher, tap = hop) | K (or Space) | A (south) |
| Attack | J (or X) | X (west) |
| Up / down attack | hold W / S + Attack. Down in the air **pogoes** off enemies, spikes and golden bells | same |
| Dash (after the Matron) | L (or Left Shift) | RT / B |
| Wall slide + wall jump (after the shrine) | hold toward the wall, then Jump | same |
| Focus: hold to heal one mask (costs 33 soul) | F (or U) | LT |
| Ember Bolt (costs 33 soul) | I (or V) | Y |
| Drop through a thin platform | S + Jump | |
| Rest at a bench | W (or Up) | Up |
| Pause / options | Esc | Start |
| Debug overlay | F1 | |

Every key can be rebound in **Options -> Controls**. Volumes, screen shake, vsync and
**Graphics** (Low / Medium / High / **Ultra**, the default) are in Options too; they are kept
in `saves/settings.ron`. If Ultra runs too slowly or looks wrong on your machine, drop to
High there (or start with `cargo play -- --quality high`). Short prompts teach the controls as they become useful and use
your own keys.

## How to read the game

Everything that can hurt you announces itself first, in one colour language:

| Colour | Meaning |
|---|---|
| yellow | it just noticed you |
| flashing orange / amber | **windup**: an attack is coming, get out of the way |
| red | the attack is happening |
| blue | recovering: **hit it now** |

* **Masks** (top left) are health; the glass vessel beside them is **soul**. Every hit
  you land fills it (11); at 33 it starts to glow, which buys one Focus heal or one
  Ember Bolt. The screen edge flashes red when you are hurt.
* **Benches** (Up to sit) heal you, refill soul, set where you wake after dying, save
  the game and bring every ordinary enemy back. Dying returns you to the last bench.
* **Bosses** seal the doorways (they glow red) until the fight is over. Amber floor
  marks show where bells will fall (they blink faster as it gets close). Golden
  swinging bells can be pogoed.

The world, in the order it opens up:

| Area | Rooms | What it teaches |
|---|---|---|
| Ashen Descent | A1 The Landing, A2 Broken Ledges, A3 First Husk, A4 The Spike Pit | mostly fighting: a training dummy and your first Husk in the very first room, walk-down platforms, a bench in A2, your first flier, one spike pit to cross (or pogo over) |
| Gutterglow Warrens | B1 the hub, B2 Husk Nest, B3 Wisp Shaft, B4 Matron's Den | a hub with four doors; the **Gutter Matron** gives you **Dash** |
| Cistern of Bells | C1 Flooded Walk, C2 Shield Gallery, C3 Bell Shaft, C4 Grip Shrine | needs Dash; the shrine teaches **Wall Grip** |
| Lantern Spire | D1 The Ascent, D2 Bell-Keeper's Rest, D3 The Gauntlet, D4 The Hollow Throne | needs Wall Grip (and both later); the **Bellwarden** |

## For developers

```
cargo play                             # the game (release build)
cargo test --workspace                 # ~200 tests, headless, a few seconds
cargo clippy --workspace --all-targets --features hk_game/dev -- -D warnings
cargo run -p hk_game --features dev    # faster rebuilds (dynamic linking; Linux/macOS)
```

**Start options** (`cargo play -- ...`): `--room ID` and `--entry NAME` start in any room
(developer start: nothing is saved; add `--all` to unlock every move; `sandbox`,
`dev_matron` and `dev_bellwarden` are test rooms), `--new` ignores the save, `--bot`
lets the boss-fight bot play, `--boss-hp-pct N` starts a boss fight late,
`--quality low|medium|high|ultra` overrides the graphics tier for one run,
`--show-hitboxes` also draws the sword's real hitbox, and `--viewer` (with `--viewer-set
knight|enemies|husk|wisp|shield|spitter|dummy|matron|warden|materials` and
`--viewer-cols 0,3,4`) lays every model, or every generated material, out with no game
running.

**Feel numbers** live in `assets/tuning/*.ron` (jump height, dash speed, coyote time,
boss attack timings, camera lead...). They are read at startup, so edit and restart:
no rebuild. `cargo run -p hk_tools --bin dump_tuning -- assets/tuning --force`
regenerates them from the code defaults.

**Level design** is authored by `python3 tools/build_rooms.py` (rectangles and
platforms instead of hand-typed ASCII) which writes `assets/rooms/*.room.ron`.
`--show B1` prints a room's map.

**Tools**

| Command | What it does |
|---|---|
| `cargo run --release -p hk_tools --bin lint_rooms` | plays the map out on paper with the real controller: every room reachable in the intended order, every gate really gates, no softlocks. `-- --map C3:west --with dash` draws where you can stand |
| `cargo run --release -p hk_tools --bin bench_sim` | simulation cost per tick (budget 100 us; it measures about 3 us) |
| `python3 tools/gen_audio.py` | regenerates every sound and music loop (`--check` analyses without writing) |

Headless screenshots (this is how the rendering was checked without a GPU):
`xvfb-run cargo run -p hk_game --features dev -- --room B3 --entry west --shots room`
writes `out/shot_room.png` (needs Mesa's software Vulkan). `--shots
telegraph,glyph,pendulum` with `--bot` captures moments of a boss fight, and `--shots
title` (with no `--room`) the title screen.

### Look and feel

Nothing is loaded from disk: every model, texture and HUD picture is built in code.

* **The knight** is an original design (an oversized bone-white bell-helm with two swept
  horns, an indigo cloak, a crimson three-piece cape that streams with his motion) with a
  breathing idle, a run cycle, squash on landing, and **the Needle**, a slim sword that is
  always visible over his shoulder, swings in three directions with a bright crescent
  trail timed to the real hitbox (a test proves the drawn blade passes through the
  hitbox on every live tick).
* **Enemies and bosses** each have their own silhouette and their own windup: the Husk
  rears back before it lunges, the Wisp squeezes small then dives, the Shieldbearer
  draws its shield in and shows a glowing weak spot, the Spitter's belly swells. The
  tell colours are unchanged and now come with a shape, so a tell is never only a colour.
* **The world** is made of real-looking materials: every surface (stone, moss, timber,
  masonry, bronze, iron, bone, chitin, flesh, cloth, leather, steel) has a generated colour,
  normal, roughness/metal/occlusion, height and glow map (`look/pbr.rs`; there are no
  downloaded textures). Stone is chiselled blocks near open air with relief that shifts
  as you move (parallax), one swelling cliff face deep in the rock; each area has its own
  wetness (puddles that mirror the light, drips that fall and splash), architecture
  (ashen nave arches, giant waxy mushrooms, cast bells, stained glass, colossal ribs),
  braziers that pool light, drifting motes, growth, rubble and stalactites.
* **Creatures** are sculpted (noise-displaced geometry with matching normals), not maths
  solids: the Husk has overlapping lacquered plates with glowing seams, thorns and a
  browed skull; the Wisp is real glass round a burning nucleus; the Shieldbearer is worn,
  riveted plate; the Spitter is wet translucent flesh; the Matron wears a hide of moss;
  the Bellwarden is cast bronze with studs and real chain links. Tells, poses and hurtboxes
  are unchanged.
* **Graphics tiers**: Low (no shadows, FXAA), Medium (shadows, SMAA, parallax stone), High
  (4096 shadows, SMAA high, ambient occlusion, a generated environment map for reflections
  and ambient colour), **Ultra** (default: 8192 soft shadows, temporal anti-aliasing with
  sharpening, top-quality ambient occlusion, volumetric haze with fires and the lantern
  glowing in it, depth of field, edge fringing, film grain).

### Layout

```
crates/hk_sim    the whole game, no rendering: 120 Hz fixed-step simulation
crates/hk_game   the Bevy app: draws and hears the simulation, menus, HUD
                   rig/      geometry (meshkit) and animation maths (pose, creature)
                   models/   knight, enemies (geo.rs: their geometry), bosses, projectiles
                   look/     materials (pbr), level stone, area kits, decor, lights, doors and
                             benches, wetness, tiers (quality), environment light (ibl), grain
                   viewer, title_scene, hud_art, tutorial, vfx
crates/hk_tools  lint_rooms, bench_sim, dump_tuning
assets/          tuning/*.ron, rooms/*.room.ron, audio/ (generated)
tools/           build_rooms.py, gen_audio.py
docs/DESIGN.md   design notes, tuning rationale, playtest checklist
```

## Verification

What has been checked by machine, and what has not:

* **Controller and combat**: exact-tick tests (coyote 10 ticks works, 11 fails; jump apex
  3.6 +/- 0.05; dash 4.1 +/- 0.1; hitstop, i-frames, pogo, soul) and replay-identical runs.
* **Enemies and bosses**: state-machine tests for every attack, fairness lints on the data
  (every tell >= 300-450 ms, every recovery >= 400 ms), and a **bot with human-like reaction
  time** that fights both bosses to calibrate difficulty (the Matron falls to a clean run
  most of the time; the Bellwarden is a real final boss).
* **World**: `lint_rooms` and `tests/world.rs` prove all 16 rooms open up in three stages
  (nothing, Dash, Dash + Wall Grip) with no softlocks, and that each gate needs its ability.
* **Rendering** was checked with software-rendered screenshots (a pose sheet for every
  model, every area, every boss moment, the title), and its geometry and animation
  maths by unit tests (every mesh is validated; poses are finite, periodic and
  distinct); **audio** by analysis (levels, clipping, loop seams), not by ears. There was
  no GPU and no speaker: motion, timing feel and frame rate on real hardware are unchecked.
* **Not verifiable here**: how it feels, whether the enemy placement is fun, whether
  the boss fights are as hard as they should be for a human, perceived latency.
