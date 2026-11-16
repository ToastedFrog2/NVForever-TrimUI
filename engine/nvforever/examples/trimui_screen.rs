//! TrimUI screen and controls check (feature `trimui`): the screen cycles through colours,
//! a pad button turns it white while held. Menu + Start quits; it also quits after 30 s.
#[path = "../src/trimui.rs"]
mod trimui;

use bevy::prelude::*;

fn main() {
    App::new()
        .add_plugins(
            DefaultPlugins
                .set(WindowPlugin {
                    primary_window: Some(Window { resolution: bevy::window::WindowResolution::new(1280, 720).with_scale_factor_override(1.0), ..default() }),
                    ..default()
                })
                .disable::<bevy::winit::WinitPlugin>(),
        )
        .add_plugins(trimui::TrimuiPlugin)
        .add_systems(Startup, |mut commands: Commands| {
            commands.spawn(Camera2d);
            commands.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(100.0), top: Val::Px(100.0),
                    padding: UiRect::all(Val::Px(30.0)),
                    ..default()
                },
                BackgroundColor(Color::srgb(0.5, 0.0, 0.5)),
            )).with_children(|parent| {
                parent.spawn((
                    Text::new("NVForever display and UI test"),
                    TextFont { font_size: FontSize::Px(36.0), ..default() },
                    TextColor(Color::WHITE),
                ));
            });
        })
        .add_systems(Update, colour)
        .run();
}

fn colour(time: Res<Time<Real>>, pads: Query<&Gamepad>, mut clear: ResMut<ClearColor>, mut exit: MessageWriter<AppExit>, mut frames: Local<u32>) {
    *frames += 1;
    let t = time.elapsed_secs();
    if *frames % 60 == 0 {
        info!("check: {} frames in {t:.1} s", *frames);
    }
    let held = pads.iter().any(|p| p.get_pressed().next().is_some());
    clear.0 = if held { Color::WHITE } else { Color::hsl((t * 60.0) % 360.0, 0.8, 0.5) };
    if t > 30.0 {
        exit.write(AppExit::Success);
    }
}
