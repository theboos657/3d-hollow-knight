# Hollow Knight 3D

A 2.5D Metroidvania in the spirit of a "Hollow Knight 3": tight combat-platforming
on a 2D gameplay plane with true 3D lighting, depth and parallax. Built in Rust with
[Bevy](https://bevy.org) 0.18.

> **Status:** work in progress. Playable today: the movement/combat **sandbox**
> with four enemy types (below). Rooms, the boss and the world are being built
> milestone by milestone.

## Play it

1. Install Rust (https://rustup.rs). On **Windows** also install the
   *Visual Studio Build Tools* with the **"Desktop development with C++"** workload.
   On Linux you need the usual Bevy libraries (`libasound2-dev libudev-dev
   libwayland-dev libxkbcommon-dev pkg-config`).
2. From this folder run:

   ```
   cargo play
   ```

   The first build takes several minutes (it compiles the whole engine); after that
   it starts quickly. Always use the optimised build for feel testing.

## Controls (sandbox)

| Action | Keyboard | Gamepad |
|---|---|---|
| Move | Arrows / WASD | D-pad / left stick |
| Jump (hold = higher) | Space / Z | A (south) |
| Attack | X / J | X (west) |
| Up / down attack | hold Up / Down + Attack (down only in the air, **pogo** off enemies and spikes) | same |
| Dash | C / Left Shift | RB / B |
| Focus (hold, heals 1 mask for 33 soul) | F | LT |
| Ember Bolt (33 soul) | V | Y |
| Drop through a thin platform | Down + Jump | |
| Debug overlay | F1 | |

Useful options: `cargo play -- --room ID` starts in another room (rooms live in
`assets/rooms/*.room.ron`; the default is `sandbox`). Feel numbers live in
`assets/tuning/*.ron` and are read at startup, so you can tweak jump height, dash
speed, camera lead and so on and just restart, with no rebuild.

The sandbox unlocks everything: try the wall-jump shaft on the left, the thin
platforms, and the spike pit on the right (pogo off the floating dummy to cross it).

Enemies (they all telegraph, and glow to show what they are doing):

| Colour | Meaning |
|---|---|
| yellow | just noticed you |
| flashing orange | **windup**: an attack is coming, get out of the way |
| red | attacking |
| blue | recovering: **hit it now** |

* **Husk** (red-brown): patrols, chases, lunges. 15 HP.
* **Shieldbearer** (teal, white plate): the plate blocks nail and bolt from the front.
  Pogo it from above, or hit it from behind while it turns slowly.
* **Wisp** (purple ball): flies above you, then dives.
* **Spitter** (green): backs away and spits a slow blob; dodge or jump it.

Everything respawns a few seconds after dying.

### Bosses

Two test arenas exist while the world is being built: `cargo play -- --room dev_matron`
(the Gutter Matron) and `cargo play -- --room dev_bellwarden` (the Bellwarden). The
boss wakes when you walk in; the doorways glow red until it is over.

They use the same colour language as enemies: **flashing amber = an attack is coming,
red = it is happening, blue = it is recovering, hit it now.** Amber floor marks show
where bells will fall (blink faster as they near). Golden swinging bells can be
pogoed. The bar at the top shows the boss's health with a notch at each phase change.

Watch the bot fight (it plays with human-like reaction time):
`cargo play -- --room dev_bellwarden --bot` (add `--boss-hp-pct 40` to start a
fight late).

## Development

```
cargo test --workspace                 # simulation tests (headless, ~90 tests)
cargo clippy --workspace --all-targets -- -D warnings
cargo run -p hk_game --features dev    # faster rebuilds (dynamic linking; Linux/macOS)
```

The simulation (`crates/hk_sim`) has no rendering dependency and runs at a fixed
120 Hz; `crates/hk_game` only draws it. All feel numbers live in `assets/tuning/*.ron`.
