//! Hardware check of the 3D compiler, scaled world target and native-size interface.
#[path = "../src/trimui.rs"]
mod trimui;
use bevy::prelude::*;

#[derive(Component)]
struct Turning;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins
            .set(WindowPlugin { primary_window: Some(Window {
                resolution: bevy::window::WindowResolution::new(1280, 720).with_scale_factor_override(1.0),
                ..default()
            }), ..default() })
            .set(bevy::render::RenderPlugin {
                render_creation: bevy::render::settings::RenderCreation::Automatic(Box::new(bevy::render::settings::WgpuSettings {
                    memory_hints: bevy::render::settings::MemoryHints::MemoryUsage, ..default()
                })), ..default()
            })
            .disable::<bevy::winit::WinitPlugin>())
        .add_plugins(nvf_assets::nif_material::NifMaterialPlugin)
        .add_plugins(trimui::TrimuiPlugin)
        .add_systems(Startup, setup)
        .add_systems(Update, update)
        .run();
}

fn setup(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, mut materials: ResMut<Assets<StandardMaterial>>, mut nif: ResMut<Assets<nvf_assets::nif_material::NifMaterial>>) {
    let mode = std::env::var("NVF_PROBE_MODE").unwrap_or_else(|_| "lit".into());
    let count: usize = std::env::var("NVF_PROBE_COUNT").ok().and_then(|s| s.parse().ok()).unwrap_or(1).clamp(1, 256);
    info!("graphics probe: mode {mode}, {count} cubes");
    commands.spawn((
        Camera3d::default(), Msaa::Off,
        Transform::from_xyz(4.0, 3.0, 5.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    let cube_mesh = meshes.add(Cuboid::new(1.5, 1.5, 1.5));
    for index in 0..count {
    let position = if count == 1 { Vec3::new(0.0, 0.75, 0.0) } else { Vec3::new((index % 16) as f32 * 1.8 - 12.0, 0.75, (index / 16) as f32 * -1.8) };
    let cube = commands.spawn((
        Mesh3d(cube_mesh.clone()),
        Transform::from_translation(position), Turning,
    )).id();
    let material = StandardMaterial { base_color: Color::srgb(0.2, 0.7, 0.3), unlit: mode == "unlit", metallic: 0.25, perceptual_roughness: 0.3, ..default() };
    if mode.starts_with("nif") {
        commands.entity(cube).insert(MeshMaterial3d(nif.add(nvf_assets::nif_material::NifMaterial { base: material, extension: nvf_assets::nif_material::NifSurface { first_person: mode.contains("first"), ..default() } })));
    } else {
        commands.entity(cube).insert(MeshMaterial3d(materials.add(material)));
    }
    }
    commands.spawn((
        Mesh3d(meshes.add(Plane3d::default().mesh().size(20.0, 20.0))),
        MeshMaterial3d(materials.add(StandardMaterial { base_color: Color::srgb(0.3, 0.3, 0.35), unlit: mode == "unlit", ..default() })),
    ));
    commands.spawn((PointLight { intensity: 800_000.0, ..default() }, Transform::from_xyz(3.0, 6.0, 3.0)));
    commands.spawn((
        Text::new(format!("NVForever graphics probe: {mode}")),
        TextFont { font_size: FontSize::Px(30.0), ..default() },
        Node { position_type: PositionType::Absolute, left: Val::Px(30.0), top: Val::Px(30.0), ..default() },
    ));
}

fn update(time: Res<Time<Real>>, mut cubes: Query<&mut Transform, With<Turning>>, mut exit: MessageWriter<AppExit>) {
    for mut transform in &mut cubes { transform.rotate_y(time.delta_secs() * 0.6); }
    let seconds = std::env::var("NVF_PROBE_SECONDS").ok().and_then(|s| s.parse().ok()).unwrap_or(45.0);
    if time.elapsed_secs() > seconds { exit.write(AppExit::Success); }
}
