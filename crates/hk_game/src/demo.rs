//! Bot-driven demo mode: `--bot` lets the boss-fight bot play (handy for
//! watching a fight, and for headless verification), `--shots a,b,c` saves a
//! screenshot the first time each named moment happens and then exits, and
//! `--boss-hp-pct N` starts a boss fight already at N % health (to reach the
//! later phases quickly).
//!
//! Moments: intro, telegraph, active, recover, transition, glyph, pendulum,
//! dying, end (the end card), title (the title screen; leave out `--room`); any other name just waits for the room to settle (`--shots room`).
//! Files land in `out/shot_<moment>.png` (`--shot-prefix P_` adds a prefix).

use bevy::prelude::*;
use bevy::render::view::screenshot::{save_to_disk, Screenshot};
use hk_sim::boss::{Boss, BossBrain, BossState, Glyph, Pendulum};
use hk_sim::bot::{find_player, Bot, BotConfig};
use hk_sim::combat::Health;
use hk_sim::input::{apply_bits, InputState};
use hk_sim::{advance_tick, SimTick};

pub struct DemoPlugin {
    pub prefix: String,
    pub bot: bool,
    pub shots: Vec<String>,
    pub boss_hp_pct: Option<f32>,
}

#[derive(Resource)]
struct Demo {
    prefix: String,
    pending: Vec<String>,
    hp_pct: Option<f32>,
}

impl Plugin for DemoPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(Demo {
            prefix: self.prefix.clone(),
            pending: self.shots.clone(),
            hp_pct: self.boss_hp_pct,
        });
        if self.bot {
            // Before the tick counter advances, so a press is stamped for the
            // tick about to run (exactly what the test harness does).
            app.add_systems(FixedUpdate, bot_drive.before(advance_tick));
        }
        app.add_systems(Update, (set_boss_hp, take_shots));
    }
}

fn bot_drive(world: &mut World, mut bot: Local<Option<Bot>>) {
    let Some(player) = find_player(world) else {
        return;
    };
    let Some(boss) = world
        .query_filtered::<Entity, With<Boss>>()
        .iter(world)
        .next()
    else {
        return;
    };
    let bot = bot.get_or_insert_with(|| Bot::new(BotConfig::default()));
    let bits = bot.decide(world, player, boss);
    let tick = SimTick(world.resource::<SimTick>().0);
    let mut input = std::mem::take(&mut *world.resource_mut::<InputState>());
    apply_bits(&mut input, bits, &tick);
    *world.resource_mut::<InputState>() = input;
}

/// Once the boss is awake, drop it to the requested health (once).
fn set_boss_hp(mut demo: ResMut<Demo>, mut q: Query<(&BossBrain, &mut Health), With<Boss>>) {
    let Some(pct) = demo.hp_pct else {
        return;
    };
    for (b, mut hp) in &mut q {
        if b.state != BossState::Sleeping {
            hp.hp = ((hp.max as f32 * pct / 100.0).round() as i32).max(1);
            demo.hp_pct = None;
        }
    }
}

fn take_shots(
    mut commands: Commands,
    mut demo: ResMut<Demo>,
    mut vtime: ResMut<Time<Virtual>>,
    brains: Query<&BossBrain>,
    glyphs: Query<&Glyph>,
    pendulums: Query<&Pendulum>,
    screen: Res<crate::menu::Screen>,
    mut exit: MessageWriter<AppExit>,
    mut frames: Local<u32>,
    mut last_shot: Local<u32>,
) {
    if demo.pending.is_empty() && *last_shot == 0 {
        return;
    }
    *frames += 1;
    if *frames == 1 {
        // A CPU renderer manages a handful of frames per second; slow the
        // simulation so a frame never skips a whole moment.
        vtime.set_relative_speed(0.25);
    }
    let in_state = |s: BossState, min: u32| brains.iter().any(|b| b.state == s && b.timer >= min);
    let mut taken = Vec::new();
    for name in &demo.pending {
        let ready = match name.as_str() {
            "intro" => in_state(BossState::Intro, 60),
            "telegraph" => in_state(BossState::Telegraph, 30),
            "active" => in_state(BossState::Active, 8),
            "recover" => in_state(BossState::Recover, 30),
            "transition" => in_state(BossState::Transition, 40),
            "dying" => in_state(BossState::Dying, 60),
            "glyph" => glyphs.iter().any(|g| g.ticks < 50),
            "pendulum" => pendulums.iter().next().is_some(),
            "end" => *screen == crate::menu::Screen::Ended,
            // The title screen (start without `--room`): once its stage has settled.
            "title" => *screen == crate::menu::Screen::Title && *frames > 90,
            _ => *frames > 45,
        };
        if ready {
            std::fs::create_dir_all("out").ok();
            commands
                .spawn(Screenshot::primary_window())
                .observe(save_to_disk(format!("out/{}shot_{name}.png", demo.prefix)));
            taken.push(name.clone());
            *last_shot = *frames;
        }
    }
    demo.pending.retain(|n| !taken.contains(n));
    let done = demo.pending.is_empty() && *frames > *last_shot + 40;
    if done || *frames > 60_000 {
        exit.write(AppExit::Success);
    }
}
