# Hollow Knight 3D

A 2.5D Metroidvania in the spirit of a "Hollow Knight 3": tight combat-platforming
on a 2D gameplay plane with true 3D lighting, depth and parallax. Built in Rust with
[Bevy](https://bevy.org) 0.18.

> **Status:** work in progress. Playable today: the movement/combat **sandbox**
> (below). Enemies, rooms, the boss and the world are being built milestone by
> milestone (see the plan in the session history / `docs/`).

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

The sandbox unlocks everything: try the wall-jump shaft on the left, the thin
platforms, and the spike pit on the right (pogo off the floating dummy to cross it).

## Development

```
cargo test --workspace                 # simulation tests (headless, ~65 tests)
cargo clippy --workspace --all-targets -- -D warnings
cargo run -p hk_game --features dev    # faster rebuilds (dynamic linking; Linux/macOS)
```

The simulation (`crates/hk_sim`) has no rendering dependency and runs at a fixed
120 Hz; `crates/hk_game` only draws it. All feel numbers live in `assets/tuning/*.ron`.
