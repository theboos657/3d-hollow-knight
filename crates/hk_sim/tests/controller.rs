//! Exact-tick tests for the player controller. Every number here comes from
//! `PlayerTuning::default()` (mirrored in assets/tuning/player.ron).

use bevy_ecs::prelude::*;
use bevy_math::Vec2;
use hk_sim::components::{SimPos, Velocity};
use hk_sim::input::Action;
use hk_sim::player::{spawn_player, Abilities, Motor, PlayerState};
use hk_sim::testing::Harness;
use hk_sim::tuning::PlayerTuning;
use hk_sim::world::{TileGrid, SKIN};

const REST_Y: f32 = 2.0 + 0.75 + SKIN; // floor top + half height + skin

fn t() -> PlayerTuning {
    PlayerTuning::default()
}

fn scene(rows: &[&str], start: Vec2, abil: Abilities) -> (Harness, Entity) {
    let mut h = Harness::new();
    h.world_mut().insert_resource(TileGrid::from_ascii(rows));
    let e = spawn_player(h.world_mut(), start, abil);
    (h, e)
}

fn pos(h: &Harness, e: Entity) -> Vec2 {
    h.world().get::<SimPos>(e).unwrap().0
}
fn vel(h: &Harness, e: Entity) -> Vec2 {
    h.world().get::<Velocity>(e).unwrap().0
}
fn motor(h: &Harness, e: Entity) -> Motor {
    h.world().get::<Motor>(e).unwrap().clone()
}
fn state(h: &Harness, e: Entity) -> PlayerState {
    *h.world().get::<PlayerState>(e).unwrap()
}

/// Flat arena: floor top at y = 2, 60 wide, open above.
fn flat() -> Vec<&'static str> {
    const AIR: &str = "............................................................";
    const FLOOR: &str = "############################################################";
    let mut v = vec![AIR; 12];
    v.push(FLOOR);
    v.push(FLOOR);
    v
}

fn flat_scene(abil: Abilities) -> (Harness, Entity) {
    let rows = flat();
    let (mut h, e) = scene(&rows, Vec2::new(10.0, REST_Y), abil);
    h.tick_n(5); // settle
    assert!(motor(&h, e).grounded, "player should start settled on the floor");
    (h, e)
}

const NONE: Abilities = Abilities { dash: false, wall_grip: false };
const DASH: Abilities = Abilities { dash: true, wall_grip: false };
const GRIP: Abilities = Abilities { dash: false, wall_grip: true };

// ---------------------------------------------------------------- basics --

#[test]
fn settles_on_the_ground_exactly() {
    let rows = flat();
    let (mut h, e) = scene(&rows, Vec2::new(10.0, 8.0), NONE);
    h.tick_n(90);
    let p = pos(&h, e);
    assert!((p.y - REST_Y).abs() < 1e-4, "rest y {}", p.y);
    assert_eq!(vel(&h, e).y, 0.0);
    assert_eq!(state(&h, e), PlayerState::Grounded);
}

#[test]
fn run_accelerates_to_full_speed_in_50ms() {
    let (mut h, e) = flat_scene(NONE);
    h.press(Action::Right);
    h.tick_n(3);
    assert!((vel(&h, e).x - 4.5).abs() < 1e-3, "half speed after 25 ms");
    h.tick_n(3);
    assert!((vel(&h, e).x - 9.0).abs() < 1e-3, "full speed after 50 ms");
    h.tick_n(30);
    assert!((vel(&h, e).x - 9.0).abs() < 1e-3, "speed holds at run speed");
}

#[test]
fn releasing_stops_in_about_40ms() {
    let (mut h, e) = flat_scene(NONE);
    h.press(Action::Right);
    h.tick_n(10);
    h.release(Action::Right);
    h.tick_n(5); // 40 ms = 4.8 ticks
    assert_eq!(vel(&h, e).x, 0.0);
}

// ------------------------------------------------------------------ jump --

fn max_height_holding_jump(hold_ticks: u32) -> (f32, u32) {
    let (mut h, e) = flat_scene(NONE);
    let y0 = pos(&h, e).y;
    h.press(Action::Jump);
    let mut max = 0.0f32;
    let mut apex_tick = 0;
    for i in 1..=90u32 {
        if i == hold_ticks + 1 {
            h.release(Action::Jump);
        }
        h.tick();
        let dy = pos(&h, e).y - y0;
        if dy > max {
            max = dy;
            apex_tick = i;
        }
    }
    (max, apex_tick)
}

#[test]
fn full_jump_apex_is_3_6_units_in_about_360ms() {
    let (apex, apex_tick) = max_height_holding_jump(200);
    assert!((apex - 3.6).abs() < 0.05, "apex {apex}");
    // 0.36 s = 43 ticks; the hang near the apex adds a couple.
    assert!((41..=48).contains(&apex_tick), "apex at tick {apex_tick}");
}

#[test]
fn tapping_jump_is_a_short_hop() {
    let (apex, _) = max_height_holding_jump(1);
    assert!(apex > 0.3 && apex < 1.3, "tap apex {apex}");
    let (full, _) = max_height_holding_jump(200);
    assert!(apex < full * 0.4, "tap must be much lower than full jump");
}

#[test]
fn holding_jump_never_rejumps() {
    let (mut h, e) = flat_scene(NONE);
    h.press(Action::Jump);
    let mut takeoffs = 0;
    let mut was_grounded = true;
    for _ in 0..400 {
        h.tick();
        let g = motor(&h, e).grounded;
        if was_grounded && !g {
            takeoffs += 1;
        }
        was_grounded = g;
    }
    assert_eq!(takeoffs, 1, "one press = one jump, however long it is held");
}

#[test]
fn fall_speed_is_capped_at_terminal_velocity() {
    let rows = flat();
    let (mut h, e) = scene(&rows, Vec2::new(10.0, 12.0), NONE);
    let mut min_vy = 0.0f32;
    for _ in 0..30 {
        h.tick();
        min_vy = min_vy.min(vel(&h, e).y);
    }
    assert!(min_vy >= -t().terminal_speed - 1e-4);
    assert!(min_vy <= -t().terminal_speed + 1.0, "should reach terminal, got {min_vy}");
}

// ----------------------------------------------------------- coyote time --

/// Walks off a ledge, then presses jump so it is evaluated `k` ticks after the
/// first airborne tick. Returns whether the jump fired.
fn jump_k_ticks_after_leaving_ledge(k: u32) -> bool {
    let mut rows = flat();
    // Cut the floor away right of x = 20 (both floor rows are the last two).
    let n = rows.len();
    rows[n - 1] = "####################........................................";
    rows[n - 2] = "####################........................................";
    let (mut h, e) = scene(&rows, Vec2::new(15.0, REST_Y), NONE);
    h.tick_n(5);
    h.press(Action::Right);
    let mut guard = 0;
    while motor(&h, e).grounded {
        h.tick();
        guard += 1;
        assert!(guard < 200, "never left the ledge");
    }
    // Tick T (first airborne tick) has just run. Evaluate the jump at T + k.
    h.tick_n(k - 1);
    h.press(Action::Jump);
    h.tick();
    vel(&h, e).y > 5.0
}

#[test]
fn coyote_jump_allowed_through_10_ticks_and_denied_at_11() {
    assert_eq!(t().coyote_ticks(), 10);
    assert!(jump_k_ticks_after_leaving_ledge(1));
    assert!(jump_k_ticks_after_leaving_ledge(10), "10th airborne tick must still jump");
    assert!(!jump_k_ticks_after_leaving_ledge(11), "11th airborne tick must not");
}

// ----------------------------------------------------------- jump buffer --

/// Drops the player, and presses jump `k` ticks before the landing tick.
/// Returns whether the player jumped on landing.
fn buffered_jump_k_ticks_before_landing(k: u32) -> bool {
    let rows = flat();
    let start = Vec2::new(10.0, 5.0);

    // Reference run: find the landing tick L (first tick with grounded).
    let (mut h, e) = scene(&rows, start, NONE);
    let mut landing = 0u64;
    for _ in 0..200 {
        h.tick();
        if motor(&h, e).grounded {
            landing = h.tick_count();
            break;
        }
    }
    assert!(landing > k as u64 + 2, "drop too short for the test");

    // Real run: press after tick (L - k).
    let (mut h, e) = scene(&rows, start, NONE);
    while h.tick_count() < landing - k as u64 {
        h.tick();
    }
    h.press(Action::Jump);
    while h.tick_count() < landing + 1 {
        h.tick();
    }
    // Tick L + 1 is when a buffered press would fire.
    vel(&h, e).y > 5.0
}

#[test]
fn jump_buffer_fires_within_12_ticks_and_expires_at_13() {
    let buf = t().jump_buffer_ticks();
    assert_eq!(buf, 12);
    assert!(buffered_jump_k_ticks_before_landing(buf), "pressed 12 ticks early: jump");
    assert!(!buffered_jump_k_ticks_before_landing(buf + 1), "13 ticks early: expired");
    assert!(buffered_jump_k_ticks_before_landing(1));
}

// ------------------------------------------------------------------ dash --

#[test]
fn ground_dash_covers_speed_times_duration() {
    let (mut h, e) = flat_scene(DASH);
    let x0 = pos(&h, e).x;
    h.press(Action::Dash);
    h.tick();
    h.release(Action::Dash);
    assert_eq!(state(&h, e), PlayerState::Dash);
    h.tick_n(t().dash_ticks() - 1);
    let dist = pos(&h, e).x - x0;
    let expected = t().dash_speed * t().dash_ticks() as f32 / 120.0; // 4.0
    assert!((dist - expected).abs() < 0.05, "dash distance {dist} vs {expected}");
    assert_ne!(state(&h, e), PlayerState::Dash);
}

#[test]
fn dash_needs_the_ability() {
    let (mut h, e) = flat_scene(NONE);
    let x0 = pos(&h, e).x;
    h.press(Action::Dash);
    h.tick_n(30);
    assert_eq!(pos(&h, e).x, x0);
}

#[test]
fn dash_cooldown_blocks_a_second_dash_until_it_ends() {
    let (mut h, e) = flat_scene(DASH);
    h.press(Action::Dash);
    h.tick();
    h.release(Action::Dash);
    h.tick_n(t().dash_ticks()); // dash over, cooldown (42 ticks from start) still running
    let x_after = pos(&h, e).x;
    h.press(Action::Dash);
    h.tick();
    h.release(Action::Dash);
    assert_ne!(state(&h, e), PlayerState::Dash, "cooldown must block the dash");
    assert!((pos(&h, e).x - x_after).abs() < 2.0);

    // Wait out the cooldown (and the buffered press, which expires), then dash.
    h.tick_n(60);
    h.press(Action::Dash);
    h.tick();
    assert_eq!(state(&h, e), PlayerState::Dash);
}

#[test]
fn dash_is_flat_and_air_dash_is_once_per_airtime() {
    let (mut h, e) = flat_scene(DASH);
    h.press(Action::Jump);
    h.tick_n(12);
    h.release(Action::Jump);
    let y_before = pos(&h, e).y;
    h.press(Action::Dash);
    h.tick();
    h.release(Action::Dash);
    assert_eq!(state(&h, e), PlayerState::Dash);
    h.tick_n(t().dash_ticks() - 1);
    assert!((pos(&h, e).y - y_before).abs() < 0.3, "no gravity during a dash");

    // Second air dash after the cooldown: denied.
    h.tick_n(30);
    if !motor(&h, e).grounded {
        h.press(Action::Dash);
        h.tick();
        assert_ne!(state(&h, e), PlayerState::Dash, "only one air dash per airtime");
        h.release(Action::Dash);
    }
    // Land, wait out the cooldown: refilled.
    while !motor(&h, e).grounded {
        h.tick();
    }
    h.tick_n(60);
    h.press(Action::Dash);
    h.tick();
    assert_eq!(state(&h, e), PlayerState::Dash);
}

#[test]
fn jumping_out_of_a_dash_keeps_boosted_momentum() {
    let (mut h, e) = flat_scene(DASH);
    h.press(Action::Right);
    h.press(Action::Dash);
    h.tick();
    h.release(Action::Dash);
    h.tick_n(4);
    h.press(Action::Jump);
    h.tick();
    let v = vel(&h, e);
    assert!(v.y > 5.0, "jump fired out of the dash");
    assert!(v.x > 9.0 * 1.2, "boosted carry speed, got {}", v.x);
}

// ------------------------------------------------------------ wall grip --

fn wall_rows() -> Vec<&'static str> {
    vec![
        "..........#",
        "..........#",
        "..........#",
        "..........#",
        "..........#",
        "..........#",
        "..........#",
        "..........#",
        "..........#",
        "..........#",
        "###########",
        "###########",
    ]
}

#[test]
fn wall_slide_caps_fall_speed_only_with_the_ability() {
    for (abil, expect_slide) in [(GRIP, true), (NONE, false)] {
        let rows = wall_rows();
        let (mut h, e) = scene(&rows, Vec2::new(9.5, 9.5), abil);
        h.press(Action::Right);
        h.tick_n(30);
        let vy = vel(&h, e).y;
        if expect_slide {
            assert!(vy >= -t().wall_slide_speed - 1e-3, "slide speed {vy}");
            assert_eq!(state(&h, e), PlayerState::WallSlide);
        } else {
            assert!(vy < -t().wall_slide_speed - 1.0, "falls freely without grip: {vy}");
        }
    }
}

#[test]
fn wall_jump_kicks_away_and_locks_input_briefly() {
    let rows = wall_rows();
    let (mut h, e) = scene(&rows, Vec2::new(9.5, 9.5), GRIP);
    h.press(Action::Right);
    h.tick_n(20);
    assert_eq!(motor(&h, e).wall, 1);
    h.press(Action::Jump);
    h.tick();
    let v = vel(&h, e);
    assert!(v.x < -8.0, "kicked left, vx = {}", v.x);
    assert!(v.y > 15.0, "vy = {}", v.y);
    // Still holding Right toward the wall: the lock keeps the kick alive.
    h.tick_n(t().wall_lock_ticks() - 2);
    assert!(vel(&h, e).x < -5.0, "input lock protects the kick");
}

#[test]
fn no_wall_jump_without_the_ability() {
    let rows = wall_rows();
    let (mut h, e) = scene(&rows, Vec2::new(9.5, 9.5), NONE);
    h.press(Action::Right);
    h.tick_n(20);
    h.press(Action::Jump);
    h.tick();
    assert!(vel(&h, e).y < 5.0);
}

// ----------------------------------------------------- one-way & corners --

#[test]
fn down_plus_jump_drops_through_a_one_way() {
    let rows = vec![
        "............",
        "............",
        "............",
        "............",
        "............",
        "....====....",
        "............",
        "............",
        "............",
        "............",
        "############",
        "############",
    ];
    // Ledge is row 5 from the top => j = 12 - 1 - 5 = 6, top surface y = 7.
    let (mut h, e) = scene(&rows, Vec2::new(5.5, 7.0 + 0.75 + SKIN), NONE);
    h.tick_n(5);
    assert!(motor(&h, e).grounded);
    assert!((pos(&h, e).y - (7.0 + 0.75 + SKIN)).abs() < 1e-3, "standing on the one-way");

    h.press(Action::Down);
    h.press(Action::Jump);
    h.tick();
    h.release(Action::Jump);
    h.release(Action::Down);
    h.tick_n(90);
    assert!((pos(&h, e).y - REST_Y).abs() < 1e-3, "ended on the floor below");
}

fn ceiling_scene(corner_correction: f32) -> (Harness, Entity) {
    // Ceiling block on columns 10..13. Its bottom edge is at y = 4 (rows 5..7
    // of 12 => j = 6..4), while the player's top starts at y = 3.5, so a jump
    // hits the ceiling after 0.5 units unless the corner is corrected.
    let rows = vec![
        "................",
        "................",
        "................",
        "................",
        "................",
        "..........###...",
        "..........###...",
        "..........###...",
        "................",
        "................",
        "############....",
        "############....",
    ];
    let mut h = Harness::new();
    h.world_mut().insert_resource(TileGrid::from_ascii(&rows));
    h.world_mut().resource_mut::<hk_sim::tuning::Tuning>().player.corner_correction =
        corner_correction;
    // Right edge of the player at x = 10.1: overlaps the block by 0.1.
    let e = spawn_player(h.world_mut(), Vec2::new(9.7, REST_Y), NONE);
    h.tick_n(5);
    (h, e)
}

#[test]
fn ceiling_corner_correction_slips_past_a_grazing_corner() {
    let (mut h, e) = ceiling_scene(0.25);
    let y0 = pos(&h, e).y;
    h.press(Action::Jump);
    let mut max_dy = 0.0f32;
    for _ in 0..80 {
        h.tick();
        max_dy = max_dy.max(pos(&h, e).y - y0);
    }
    assert!(max_dy > 3.4, "corrected jump reaches full height, got {max_dy}");
    assert!(pos(&h, e).x < 9.7 + 1e-3 && pos(&h, e).x > 9.7 - 0.3);
}

#[test]
fn without_correction_the_same_jump_bonks() {
    let (mut h, e) = ceiling_scene(0.0);
    let y0 = pos(&h, e).y;
    h.press(Action::Jump);
    let mut max_dy = 0.0f32;
    for _ in 0..80 {
        h.tick();
        max_dy = max_dy.max(pos(&h, e).y - y0);
    }
    assert!(max_dy < 0.6, "bonked under the block, got {max_dy}");
}

// ----------------------------------------------------------- determinism --

fn scripted_run() -> Vec<(f32, f32)> {
    let (mut h, e) = flat_scene(DASH);
    let mut rng = hk_sim::rng::SimRng::new(99);
    let mut trace = Vec::new();
    for tick in 0..1500 {
        if tick % 7 == 0 {
            h.set(Action::Right, rng.chance(0.6));
            h.set(Action::Left, rng.chance(0.2));
            h.set(Action::Jump, rng.chance(0.4));
            h.set(Action::Dash, rng.chance(0.15));
        }
        h.tick();
        let p = pos(&h, e);
        trace.push((p.x, p.y));
    }
    trace
}

#[test]
fn identical_input_gives_bit_identical_motion() {
    assert_eq!(scripted_run(), scripted_run());
}
