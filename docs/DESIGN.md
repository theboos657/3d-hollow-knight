# Design notes

How the game is built, why the numbers are what they are, what has been proven and
what has not, and what to look at first when you play it.

## 1. Pillars

1. **Tight.** Input to motion in as few milliseconds as the hardware allows. Jumps, dashes
   and swings do exactly the same thing every time.
2. **Fair.** Everything that can hurt you announces itself first, the same way every time
   (colour language, minimum tell times enforced as data lints), and leaves a window to punish it.
3. **Interconnected.** A hub, four areas, doors that open when you learn to do something new,
   and a shortcut back.
4. **Original.** The mechanics are the genre's; the names, creatures, art and sound are made here.

## 2. Architecture

Rust + Bevy 0.18. The important decision is the split:

```
hk_sim   (no renderer)                      hk_game (Bevy app)
  120 Hz fixed-step simulation   <---reads---  interpolation, camera, lights, meshes,
  input latch, controller, combat,             VFX, HUD, menus, audio, save files
  enemies, bosses, rooms, progress
```

* **2.5D.** Gameplay lives on the z = 0 plane (1 tile = 1 world unit); art, lighting,
  parallax and the camera are true 3D. A free 3D arena would need lock-on cameras and
  cost the tightness; the genre's combat language (4-way nail, pogo, dash) is 2D reasoning.
* **No physics engine.** A swept-AABB tile collider (X then Y, sub-steps under 0.4 u, a 1 mm
  skin) is deterministic, tunnel-free at every speed in the game (max ~24 u/s = 0.2 u/tick)
  and small enough to read in one sitting. The same code runs in the game, the tests, the bots
  and the reachability analysis.
* **Fixed 120 Hz tick, interpolated rendering.** Worst-case input quantisation is 8.3 ms.
  Visuals lerp `PrevPos -> SimPos` by how far into the next tick the frame is, and the camera
  follows the *interpolated* position (no jitter). The tick is single-threaded and ordered:

  ```
  Input -> Intent (player, enemy and boss decisions) -> Motion -> Collision
        -> HitDetect -> HitResolve (damage, soul, pogo, knockback, hitstop)
        -> Status (timers, i-frames, death, benches, pickups) -> Cleanup
  ```
* **Input latch.** Bevy clears `just_pressed` every rendered frame but the sim runs 0, 1 or
  2+ ticks per frame. Every press edge is stamped with the first tick allowed to see it and
  stays buffered until a system consumes it or it ages out, so a tap can never be lost, and
  buffered jump/dash/attack windows are measured in ticks.
* **Hitstop** is a global gate on Intent..Status while input keeps latching, so a press made
  during a freeze frame still fires when time resumes.
* **Data-driven.** Feel numbers are RON (`assets/tuning`), authored in milliseconds and
  converted with `round(ms * 0.12)`. Tests assert the shipped files equal the code defaults.
* **Determinism.** One seeded RNG in the sim, fixed system order: the same inputs give
  bit-identical results (tests replay whole fights).

## 3. The controller

| Knob | Value | Why |
|---|---|---|
| Run speed | 9 u/s, 50 ms to full | near-instant, like the genre |
| Full jump | 3.6 u apex in 0.36 s (g up = 55.6, v0 = 20) | derived from height + time, not guessed |
| Fall gravity | x1.6, terminal 24 u/s | a heavy fall feels snappy |
| Variable jump | release while rising: vy x0.4 | low hops without a second button |
| Apex hang | gravity x0.6 when |vy| < 2 | a moment to aim, tunable |
| Coyote time | 80 ms (10 ticks) | ledge grace |
| Jump / dash / attack buffer | 100 ms | a press just before landing still counts |
| Dash | 24 u/s for 170 ms (~4.1 u), cooldown 350 ms, 120 ms of i-frames, one in the air per airtime | dash-jump keeps momentum (x1.35) |
| Wall slide / jump | 3.5 u/s slide; jump vx 9, vy 18, 120 ms input lock | gated ability |
| Corner correction | up to 0.25 u | a head bump slips past a ceiling corner |

What a plain jump can do (measured by the analysis tool, not assumed): clear a gap of
**7 tiles**, or a bed of spikes **4 tiles** wide; with Dash, **10** and **8**. So the world's
dash gates are 6-tile spike beds (C1, D3) and an 8-tile hop (C3): too far for a jump, well
within a dash.

## 4. Combat

* **Nail:** 5 damage; startup 30 ms, active 90 ms, cooldown 350 ms; forward reach 2.2 x 1.4,
  vertical 1.6 x 2.2; a hit gives a small recoil and 50 ms of hitstop.
* **Pogo:** a down-slash that lands on anything pogoable (enemies, spikes, golden bells)
  gives vy = 16 and refills your air dash, once per swing.
* **Hurt:** one mask, 200 ms stun, knockback (12, 8), **1.3 s of i-frames**, 120 ms hitstop,
  camera shake. Five masks.
* **Soul:** +11 per nail hit (max 99). **Focus** (hold 1 s, 33 soul) heals one mask and breaks
  if you are hit. **Ember Bolt** (33 soul, 15 damage, 16 u/s).
* **Spikes:** one mask, then you are put back on the last spot you stood on that was at
  least 1.5 u from a hazard (so a respawn can't drop you back on the spikes).

## 5. Enemies and bosses

Every creature is a small state machine with a telegraph as a first-class state:

| Creature | Behaviour | Tell |
|---|---|---|
| Husk (15 HP) | patrols, notices you (300 ms), chases, lunges | 350 ms windup |
| Wisp (10 HP) | hovers above you, dives | 400 ms |
| Shieldbearer (25 HP) | the plate blocks nail and bolt from the front: pogo it or hit its back | bash windup |
| Spitter | keeps its distance and spits a slow blob | 500 ms |

**Fairness rules**, enforced as data lints in the tests: every enemy windup >= 300 ms; every
boss's first tell >= 450 ms and its recovery >= 400 ms in the snappiest phase; a combo's
follow-up has a >= 300 ms tell of its own; attack choice is weighted and range/phase-aware
with no attack more than twice in a row.

**The Gutter Matron** (300 HP, one phase, guards Dash): *Toll Slam* (600 ms tell, leaps and
lands with two ground shockwaves) and *Warden's Charge* (500 ms tell, 16 u/s, stunned if it
hits a wall: a big punish window).

**The Bellwarden** (350 HP, three phases at 65 % and 30 %; a 1.5 s invulnerable roar between):

| Attack | Tell | Notes |
|---|---|---|
| Toll Slam | 650 ms | leap and slam, two shockwaves you jump or dash |
| Warden's Charge | 550 ms | 20 u/s |
| Falling Bells | 800 ms | amber floor marks, then bells drop: stand in the gaps |
| Falling Bells II (phase 2+) | 800 ms | five bells, tighter gaps |
| Chain Sweep (phase 2+) | 450 ms + 400 ms | two arcs; the second has its own tell |
| Pendulum Bells (phase 2+) | 700 ms | swinging golden bells: a pogo puzzle |
| Final Toll (phase 3) | 800 ms | three rounds of shockwaves along the floor both ways, 600 ms apart, then a long recovery |

**Calibration.** A boss bot with human-like limits (250 ms awareness delay, current
kinematics, a tunable mistake rate) fights each boss 16 times per setting. Latest numbers
(`cargo test -p hk_sim --test boss_bot report -- --ignored --nocapture`):

| Bot | Matron wins | Bellwarden wins |
|---|---|---|
| clean (0 % mistakes) | 15 / 16 | 9 / 16 |
| 5 % mistakes | 13 / 16 | 3 / 16 |
| 10 % mistakes | 6 / 16 | 1 / 16 |

The bot almost never heals, which a person will, so a person should do better than these
numbers; but they say the Matron is a fair mid-boss and the Bellwarden is a real test.
Treat it as a starting point for your own playtest, not a verdict.

## 6. The world

```
A1 --hole--> A2 --> A3 --> A4 --> B1 (hub, bench) --E--> B2 --> B3 --> B4  Matron -> DASH
                                   |  \--floor hatch--> C1 --> C2 --> C3 --> C4  shrine -> WALL GRIP
                                   \--N door------------> D1 --> D2 --> D3 --> D4  Bellwarden
                                        D2 --chute (one way)--> back to B1
```

| Gate | What stops you | What opens it |
|---|---|---|
| C1 (Flooded Walk), east door | 6 tiles of spikes | Dash |
| C3 (Bell Shaft), first hop | an 8-tile gap | Dash |
| C4 (Grip Shrine), way out | the drop into the shrine is a one-way trip until you have the pickup | Wall Grip |
| D1 (The Ascent) | a 60-tile shaft | Wall Grip |
| D3 (The Gauntlet) | spikes *and* an 8-tall wall | both |

Benches: A3, B1, B3 (top, right before the Matron), C2, C3 (top), D2, D3 (right before the
throne). Dying in a boss fight puts you back at the bench before it, and the arena resets.
Boss defeats and pickups persist and are saved.

**Proof, not just intent.** `lint_rooms` runs the real controller over every room (walk, jumps
at four heights, edge jumps at the last moment, dashes at five timings, wall climbs of two kinds,
drop-throughs) from every standing spot it finds, follows every exit, collects abilities in
order, and checks: all 16 rooms are reachable in three stages (nothing, Dash, Dash + Wall Grip);
no exit, pickup or bench is dead; no softlocks; and per-room tests show each gate needs its
ability. Its moves are a subset of what a person can do (no pogo tricks, no frame-perfect
routes), so it can call something unreachable that an expert can do, never the reverse.

## 7. Camera

A perspective camera (FOV 38, 23.2 u away, about 16 u visible height at the play plane).
Critically damped follow (x 120 ms, y 250 ms) with a vertical dead zone so jumps don't bob the
view; a 3.5 u lookahead that flips only after 250 ms of consistent direction; hold Up/Down for
0.5 s to look 4 u; a look-down that grows as you fall; the frustum footprint at z = 0 is clamped
to the room (rooms smaller than the view are centred); trauma-based shake (translation only).
In a boss fight it leans toward the boss so both stay in frame. It is a pure function with its
own tests (bounds never violated, hysteresis, no jitter).

## 8. Audio

`tools/gen_audio.py` synthesises 26 effects and 8 loops (a bell-and-drone palette) with the
standard library only; deterministic, and `--check` analyses peaks, DC offset and loop seams.
The game plays effects from what the simulation reports (jumps and landings from state changes,
hits/blocks/deaths from messages, enemy and boss tells) and music by place: an area theme, or a
boss track while a fight seals the room, crossfaded. Volume sliders are in Options.

## 9. What has been verified, and what has not

Proven by tests: the controller's exact behaviour, combat rules, every enemy and boss attack,
data lints, the room format, transitions, benches, saves, menus (with simulated key presses),
and world reachability. Measured: the simulation costs about 3 us per tick (budget 100 us; a
tick has 8333 us).

Not verifiable in the environment this was built in (no GPU, no speakers, no human): **feel**,
how the rooms *play* (enemy placement, pacing), how hard the bosses are for a person, how the
music sounds, perceived input latency, and behaviour on your GPU/OS.

## 10. Playtest checklist

Play once through without reading anything; write down where you were confused, bored or angry.

**Feel**
- [ ] Do the first three jumps in A1/A2 feel right? (height, weight, landing)
- [ ] Is a tap a hop and a hold a full jump? Any missed or eaten inputs?
- [ ] Does the nail feel fast and crunchy? Is the hitstop pleasant or sticky?
- [ ] Pogo: can you chain it reliably over the spikes in A4?
- [ ] Dash and wall jump: readable, controllable, satisfying?
- [ ] Camera: does it ever lag, bob, or hide the thing that's about to hit you?

**Difficulty**
- [ ] Is A3's first Husk a lesson or a wall? Can you always tell what hit you?
- [ ] Matron: how many tries? Which attack killed you most? Was each tell readable?
- [ ] Bellwarden: same. Was phase 2 or 3 the spike? Any attack that felt unfair?
- [ ] Are benches where you wanted them? Was any walk-back too long?

**Level design**
- [ ] Did you always know where to go next? Where did you get lost?
- [ ] Did the gates read as gates (a gap you can't cross yet), or did you feel stuck?
- [ ] Is anything tedious (B3's climb, C3's climb, D1's shaft, D3)?

**Presentation**
- [ ] Is every room readable (floor vs. background, platforms vs. walls)?
- [ ] Sounds: anything too loud, harsh, repetitive, or missing?

### The ten numbers to turn first

All in `assets/tuning/*.ron`, no rebuild needed.

1. `player.ron` `jump_height` (3.6) and `jump_time_ms` (360): the whole feel of vertical movement.
2. `player.ron` `fall_gravity_mult` (1.6): floaty vs. snappy.
3. `player.ron` `coyote_ms` (80) and `jump_buffer_ms` (100): forgiveness.
4. `player.ron` `dash_speed` / `dash_ms` / `dash_cooldown_ms` (24 / 170 / 350).
5. `combat.ron` `nail_cooldown_ms` (350) and `hitstop_nail_ms` (50): swing rhythm and crunch.
6. `combat.ron` `iframes_ms` (1300) and `max_masks` (5): how forgiving damage is.
7. `combat.ron` `soul_per_hit` (11): how often you can heal.
8. `enemies.ron` husk `windup_ms` (350), `chase_speed` (4.5): the first enemy's difficulty.
9. `bosses.ron` the Bellwarden's `hp` (350) and each attack's `recover_ms`: length and punish windows.
10. `camera.ron` `follow_x_ms` (120) and `lookahead` (3.5): how the view leads you.

(Changing the jump or dash numbers can invalidate the level design: re-run `lint_rooms` and
`cargo test -p hk_sim --test reach`, which say which gates no longer gate.)

## 11. Known limitations

* Art is procedural boxes and lights. It is readable, not pretty.
* Rebinding is keyboard only; the gamepad layout is fixed.
* One save slot. No map screen.
* Music and sound are generated and simple; they are placeholders in the honest sense.
* The Bellwarden's difficulty is calibrated against a bot, not a person.
