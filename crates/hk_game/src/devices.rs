//! Keyboard + gamepad -> `InputState`. Sampled right before the fixed-tick
//! loop each frame so the sim sees the freshest possible state.

use bevy::prelude::*;
use hk_sim::input::{Action, InputState};
use hk_sim::SimTick;

pub struct DevicePlugin;

impl Plugin for DevicePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            RunFixedMainLoop,
            sample_devices
                .in_set(RunFixedMainLoopSystems::BeforeFixedMainLoop)
                .run_if(crate::menu::is_playing),
        );
    }
}

const STICK_DEADZONE: f32 = 0.4;

fn sample_devices(
    keys: Res<ButtonInput<KeyCode>>,
    pads: Query<&Gamepad>,
    settings: Res<crate::settings::Settings>,
    tick: Res<SimTick>,
    mut input: ResMut<InputState>,
) {
    let next = tick.0 + 1;
    for action in Action::ALL {
        let action = &action;
        let codes = settings.key_codes(*action);
        let tapped = codes.iter().any(|k| keys.just_pressed(*k));
        let down = codes.iter().any(|k| keys.pressed(*k));
        // A press and release inside one frame leaves `pressed` false; latch
        // the edge anyway so a fast tap is never lost.
        if tapped && !down {
            input.set(*action, true, next);
        }
        let mut down = down;

        for pad in &pads {
            down |= match action {
                Action::Left => {
                    pad.pressed(GamepadButton::DPadLeft) || pad.left_stick().x < -STICK_DEADZONE
                }
                Action::Right => {
                    pad.pressed(GamepadButton::DPadRight) || pad.left_stick().x > STICK_DEADZONE
                }
                Action::Up => {
                    pad.pressed(GamepadButton::DPadUp) || pad.left_stick().y > STICK_DEADZONE
                }
                Action::Down => {
                    pad.pressed(GamepadButton::DPadDown) || pad.left_stick().y < -STICK_DEADZONE
                }
                Action::Jump => pad.pressed(GamepadButton::South),
                Action::Attack => pad.pressed(GamepadButton::West),
                Action::Dash => {
                    pad.pressed(GamepadButton::RightTrigger) || pad.pressed(GamepadButton::East)
                }
                Action::Focus => pad.pressed(GamepadButton::LeftTrigger),
                Action::Cast => pad.pressed(GamepadButton::North),
            };
        }
        input.set(*action, down, next);
    }
}
