//! Debug overlay (F1) and the scripted `--smoke-test` run used for headless
//! verification under a software Vulkan driver.

use bevy::diagnostic::{DiagnosticsStore, FrameTimeDiagnosticsPlugin};
use bevy::prelude::*;
use bevy::render::view::screenshot::{save_to_disk, Screenshot};
use hk_sim::input::{Action, InputState};
use hk_sim::SimTick;

pub struct DebugPlugin {
    pub smoke: bool,
}

impl Plugin for DebugPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(FrameTimeDiagnosticsPlugin::default())
            .add_systems(Startup, spawn_overlay)
            .add_systems(Update, update_overlay);
        if self.smoke {
            app.add_systems(Update, smoke_script);
        }
    }
}

#[derive(Component)]
struct Overlay;

fn spawn_overlay(mut commands: Commands) {
    commands.spawn((
        Overlay,
        Text::new(""),
        TextFont {
            font_size: 16.0,
            ..default()
        },
        TextColor(Color::srgb(0.75, 1.0, 0.8)),
        Node {
            position_type: PositionType::Absolute,
            top: Val::Px(8.0),
            left: Val::Px(10.0),
            ..default()
        },
    ));
}

fn update_overlay(
    keys: Res<ButtonInput<KeyCode>>,
    diag: Res<DiagnosticsStore>,
    tick: Res<SimTick>,
    input: Res<InputState>,
    mut q: Query<(&mut Text, &mut Visibility), With<Overlay>>,
) {
    let Ok((mut text, mut vis)) = q.single_mut() else {
        return;
    };
    if keys.just_pressed(KeyCode::F1) {
        *vis = match *vis {
            Visibility::Hidden => Visibility::Inherited,
            _ => Visibility::Hidden,
        };
    }
    let fps = diag
        .get(&FrameTimeDiagnosticsPlugin::FPS)
        .and_then(|d| d.smoothed())
        .unwrap_or(0.0);
    let held: String = Action::ALL
        .iter()
        .filter(|a| input.held(**a))
        .map(|a| format!("{a:?} "))
        .collect();
    **text = format!("tick {:>7}  fps {:>5.0}\nheld: {held}", tick.0, fps);
}

/// Scripted run for headless verification. Driven by simulation ticks (not
/// frames) so the result is identical whether the renderer runs at 8 fps on a
/// CPU or 240 fps on a GPU: walk right for ~0.6 s, stop, screenshot, quit.
fn smoke_script(
    mut commands: Commands,
    tick: Res<SimTick>,
    mut input: ResMut<InputState>,
    mut exit: MessageWriter<AppExit>,
    mut frames: Local<u32>,
    mut shot_frame: Local<Option<u32>>,
) {
    *frames += 1;
    let t = tick.0;
    input.set(Action::Right, (30..100).contains(&t), t + 1);

    if t >= 160 && shot_frame.is_none() {
        std::fs::create_dir_all("out").ok();
        commands
            .spawn(Screenshot::primary_window())
            .observe(save_to_disk("out/smoke.png"));
        *shot_frame = Some(*frames);
    }
    match *shot_frame {
        // Give the async screenshot writer time to finish before quitting.
        Some(f) if *frames > f + 30 => {
            exit.write(AppExit::Success);
        }
        // Never hang forever if ticks stop advancing.
        None if *frames > 5000 => {
            exit.write(AppExit::error());
        }
        _ => {}
    }
}
