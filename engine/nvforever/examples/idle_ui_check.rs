//! Headless comparison against Bevy's uncached layout. No game data or GPU needed.
//! cargo run -p nvforever --example idle_ui_check --no-default-features
#[path = "../src/trimui_idle.rs"]
mod trimui_idle;

use bevy::prelude::*;
use bevy::camera::{ComputedCameraValues, RenderTargetInfo};
use bevy::app::{HierarchyPropagatePlugin, PropagateSet, TaskPoolPlugin};
use bevy::text::{EmSize, RemSize};
use bevy::ui::{ComputedUiRenderTargetInfo, ComputedUiTargetCamera, ContentSize, FixedNode, IgnoreScroll, LayoutConfig, UiSystems, Val2};

#[derive(Resource, Default)]
struct Runs(usize, usize);
struct Scene { app: App, camera: Entity, root: Entity, child: Entity, other: Entity }

fn scene(cached: bool) -> Scene {
    let mut app = App::new();
    app.add_plugins(TaskPoolPlugin::default())
        .add_plugins(HierarchyPropagatePlugin::<ComputedUiTargetCamera>::new(PostUpdate))
        .add_plugins(HierarchyPropagatePlugin::<ComputedUiRenderTargetInfo>::new(PostUpdate))
        .init_resource::<bevy::ui::UiScale>()
        .init_resource::<bevy::ui::ui_surface::UiSurface>()
        .init_resource::<bevy::text::TextPipeline>()
        .init_resource::<bevy::text::FontCx>()
        .init_resource::<bevy::text::ScaleCx>()
        .init_resource::<RemSize>()
        .init_resource::<Runs>()
        .configure_sets(PostUpdate, (UiSystems::Prepare, UiSystems::Propagate, UiSystems::Content, UiSystems::Layout, UiSystems::Stack).chain())
        .configure_sets(PostUpdate, PropagateSet::<ComputedUiTargetCamera>::default().in_set(UiSystems::Propagate))
        .configure_sets(PostUpdate, PropagateSet::<ComputedUiRenderTargetInfo>::default().in_set(UiSystems::Propagate))
        .add_systems(PostUpdate, (
            bevy::ui::update::propagate_ui_target_cameras.in_set(UiSystems::Prepare),
            bevy::ui::sync_font_size_to_em_size.in_set(UiSystems::Content),
            bevy::ui::ui_layout_system.in_set(UiSystems::Layout),
            (|mut n: ResMut<Runs>| n.0 += 1).in_set(UiSystems::Layout),
            (|mut n: ResMut<Runs>| n.1 += 1).in_set(UiSystems::Stack),
        ));
    if cached { app.add_plugins(trimui_idle::IdleUiPlugin); }
    let camera = app.world_mut().spawn((Camera2d, Camera {
        computed: ComputedCameraValues { target_info: Some(RenderTargetInfo { physical_size: UVec2::new(800, 480), scale_factor: 1. }), ..default() },
        ..default()
    })).id();
    let root = app.world_mut().spawn(Node { width: percent(100), height: percent(100), overflow: Overflow::scroll(), ..default() }).id();
    let child = app.world_mut().spawn((Node { width: px(240), height: px(700), flex_shrink: 0., ..default() }, ChildOf(root))).id();
    let other = app.world_mut().spawn(Node { width: px(300), height: px(400), ..default() }).id();
    Scene { app, camera, root, child, other }
}

fn compare(a: &mut Scene, b: &mut Scene, what: &str) {
    a.app.update(); b.app.update();
    for (x, y) in [(a.root, b.root), (a.child, b.child), (a.other, b.other)] {
        assert_eq!(a.app.world().get::<ComputedNode>(x), b.app.world().get::<ComputedNode>(y), "geometry: {what}");
        assert_eq!(a.app.world().get::<UiGlobalTransform>(x), b.app.world().get::<UiGlobalTransform>(y), "transform: {what}");
    }
}

fn main() {
    assert_ne!(std::env::var("NVF_IDLE_UI").as_deref(), Ok("0"), "unset NVF_IDLE_UI for this check");
    let mut a = scene(false); let mut b = scene(true);
    compare(&mut a, &mut b, "initial");
    compare(&mut a, &mut b, "settle");
    let before = (b.app.world().resource::<Runs>().0, b.app.world().resource::<Runs>().1);
    for _ in 0..20 { compare(&mut a, &mut b, "idle"); }
    assert_eq!(before, (b.app.world().resource::<Runs>().0, b.app.world().resource::<Runs>().1), "idle frames must skip both sets");
    for step in 0..21 {
        let before = (b.app.world().resource::<Runs>().0, b.app.world().resource::<Runs>().1);
        for s in [&mut a, &mut b] {
            let w = s.app.world_mut();
            match step {
                0 => w.get_mut::<Node>(s.child).unwrap().width = px(280),
                1 => { w.entity_mut(s.root).insert(ScrollPosition(Vec2::new(0., 40.))); },
                2 => { w.entity_mut(s.child).insert(Outline { width: px(4), ..default() }); },
                3 => { w.entity_mut(s.child).remove::<Outline>(); },
                4 => { w.entity_mut(s.child).insert(UiTransform::from_translation(Val2::px(12., 23.))); },
                5 => { w.entity_mut(s.child).insert(ChildOf(s.other)); },
                6 => { w.entity_mut(s.child).insert(FixedNode); },
                7 => { w.entity_mut(s.child).remove::<FixedNode>(); },
                8 => { w.entity_mut(s.child).insert((LayoutConfig::default(), IgnoreScroll::default())); },
                9 => { w.entity_mut(s.child).remove::<(LayoutConfig, IgnoreScroll)>(); },
                10 => w.resource_mut::<RemSize>().0 = 24.,
                11 => w.get_mut::<EmSize>(s.child).unwrap().0 = 30.,
                12 => { w.get_mut::<ContentSize>(s.child).unwrap().set_changed(); },
                13 => { w.get_mut::<Camera>(s.camera).unwrap().computed.target_info.as_mut().unwrap().physical_size = UVec2::new(1000, 600); },
                14 => { w.spawn((Node::default(), ChildOf(s.root))); },
                15 => { w.entity_mut(s.root).remove::<ScrollPosition>(); },
                16 => { w.entity_mut(s.child).insert(ZIndex(4)); },
                17 => { w.entity_mut(s.child).remove::<ZIndex>(); },
                18 => { w.entity_mut(s.child).insert(GlobalZIndex(8)); },
                19 => { w.entity_mut(s.child).remove::<GlobalZIndex>(); },
                20 => { w.entity_mut(s.child).despawn(); },
                _ => unreachable!(),
            }
        }
        compare(&mut a, &mut b, &format!("mutation {step}"));
        let runs = b.app.world().resource::<Runs>();
        assert!(runs.0 > before.0 && runs.1 > before.1, "mutation {step} must invalidate both cached sets");
        compare(&mut a, &mut b, "settle after mutation");
    }
    println!("PASS: identical geometry for 21 mutations; 20 idle frames skipped layout and stack");
}
