//! The draw-distance sliders of the settings page (Display; owner request 2026-10-16):
//! how close to the camera objects, actors and items must be to be drawn, how far the
//! sun's shadows reach and how far grass grows, each as a part of its full distance. At
//! 100% each draws as before the sliders; 50% draws to half the distance, and so on.
//!
//! The full distances [design]:
//! - **Objects, actors, items**: nothing culled them before but their cells, so the full
//!   distance is the farthest a loaded cell reaches, `(--radius + 1)` cells along both
//!   axes from the camera's cell (the diagonal). A mesh of a placed reference is drawn
//!   while its origin is within the slider's part of that, and dithers out over the
//!   [`FADE`] part beyond it. Actors are the NPCs and creatures (their bodies, worn and
//!   held items); items are the references whose base is an item a player can take
//!   (weapons, armour, ammo, aid, books, notes, keys, mods, chips, caps, cards) and the
//!   items dropped in the game; objects are every other placed model of a loaded cell.
//!   Imposters, distant LOD, terrain, water and trees are not touched.
//! - **Shadows**: `--shadow-distance` (the sun's shadow cascades end at the slider's part
//!   of it).
//! - **Grass**: `--grass-distance` and `--grass-fade-range` together.
//!
//! How it culls: Bevy's visibility range, which cell fades (`fade.rs`) and door fades
//! (`door_fade.rs`) already give every streamed mesh. A mesh at rest (fully faded in)
//! gets the range [`fade::drawn_within`] its class's distance instead of the open-ended
//! one; one still fading is left to its fade and gets its range when the fade ends. A
//! mesh beyond its range is culled on the CPU too, and casts no shadow then. At 100% the
//! range is the open-ended one again, so nothing changes against the sliders' absence.
//!
//! The sliders are kept in `NVForever.ini` [Settings] like the page's switches
//! (`nvf_play::settings`).

use crate::fade;
use crate::grass::GrassConfig;
use crate::limbs::ActorRig;
use crate::sky::SkyState;
use crate::world::{CellEntity, WorldData};
use crate::Args;
use bevy::camera::primitives::Aabb;
use bevy::camera::visibility::VisibilityRange;
use bevy::prelude::*;
use fnv_world::placed::{base_object, placement};
use crate::world::LEAD_SLACK;
use fnv_world::CELL_SIZE;
use nvf_assets::NifMaterial;
use nvf_play::dropped::DroppedRoot;
use nvf_play::RefVisual;
use std::collections::HashMap;

/// The part of a mesh's draw distance over which it dithers out beyond it [design].
pub const FADE: f32 = 0.1;

/// The sliders' least value and step.
pub const MIN: f32 = 0.1;
pub const STEP: f32 = 0.05;

/// The base object types that count as items [documented: the item records, as
/// `nvf_play::world_sounds`' pick-up sounds; MISC is the clutter a player can take].
const ITEM_SIGS: [&[u8; 4]; 12] = [b"MISC", b"KEYM", b"WEAP", b"ARMO", b"AMMO", b"ALCH", b"BOOK", b"NOTE", b"IMOD", b"CHIP", b"CCRD", b"CMNY"];

/// What a reference's mesh counts as for the sliders.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DrawClass {
    Object,
    Actor,
    Item,
    /// Not under a loaded cell (an imposter): drawn as before.
    Unranged,
}

/// The sliders' values (each a part of its full distance, [`MIN`] to 1) and the full
/// distances (module notes).
#[derive(Resource, Clone, Debug)]
pub struct DrawDistances {
    pub objects: f32,
    pub actors: f32,
    pub items: f32,
    pub shadows: f32,
    pub grass: f32,
    /// The farthest a loaded cell reaches from the camera (game units).
    pub reach: f32,
    /// `--shadow-distance`.
    pub shadow_full: f32,
    /// `--grass-distance` and `--grass-fade-range`.
    pub grass_full: (f32, f32),
    /// `NVF_SIZE_CULL` (0: off): an object or item mesh is drawn no farther than this
    /// many times the radius of its bounds (half its box's diagonal, in the world's
    /// scale), within its class's distance too. A cup of 5 units is gone past a few
    /// hundred, a house of 1000 is drawn as far as its cell. On the handheld the frame
    /// is set by how many meshes are drawn, not by their pixels (docs/trimui.md).
    pub size_cull: f32,
}

impl DrawDistances {
    pub fn new(args: &Args) -> DrawDistances {
        DrawDistances {
            objects: 1.0,
            actors: 1.0,
            items: 1.0,
            shadows: 1.0,
            grass: 1.0,
            reach: full_reach(args.radius, crate::world::cell_lead()),
            shadow_full: args.shadow_distance.max(0.0),
            grass_full: (args.grass_distance.max(0.0), args.grass_fade_range.max(0.0)),
            size_cull: std::env::var("NVF_SIZE_CULL").ok().and_then(|v| v.parse::<f32>().ok()).unwrap_or(0.0).max(0.0),
        }
    }

    /// The slider's part for a class (1 for one not ranged).
    pub fn part(&self, class: DrawClass) -> f32 {
        match class {
            DrawClass::Object => self.objects,
            DrawClass::Actor => self.actors,
            DrawClass::Item => self.items,
            DrawClass::Unranged => 1.0,
        }
    }

    /// How far a class is drawn; `None`: as far as its cells (the full slider).
    pub fn distance(&self, class: DrawClass) -> Option<f32> {
        let part = self.part(class);
        (part < 1.0).then_some(self.reach * part)
    }

    /// How far a mesh of a class whose bounds have this radius is drawn: its class's
    /// distance, and no more than [`DrawDistances::size_cull`] radii for objects and
    /// items; `None`: as far as its cells.
    pub fn sized_distance(&self, class: DrawClass, radius: Option<f32>) -> Option<f32> {
        let by_size = match (class, radius) {
            (DrawClass::Object | DrawClass::Item, Some(r)) if self.size_cull > 0.0 => Some((r * self.size_cull).max(1.0)),
            _ => None,
        };
        match (self.distance(class), by_size) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
    }
}

/// The farthest a loaded cell reaches from the camera with `radius` cells loaded round
/// its cell: `radius + 1` cells along both axes. With a cell lead (`world.rs`
/// `Streamer::lead`) the next ring is loaded too, each of its cells while its nearest
/// point is within the lead and [`LEAD_SLACK`] of the camera: one cell more along both
/// axes, and no more than that distance and a cell's diagonal.
pub fn full_reach(radius: i32, lead: f32) -> f32 {
    let diagonal = CELL_SIZE * std::f32::consts::SQRT_2;
    let square = (radius.max(0) + 1) as f32 * diagonal;
    if lead > 0.0 { (square + diagonal).min(lead + LEAD_SLACK + diagonal).max(square) } else { square }
}

pub struct DrawDistancePlugin(pub DrawDistances);

impl Plugin for DrawDistancePlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(self.0.clone())
            .insert_resource(nvf_play::perk_marks::ActorDrawDistance(self.0.distance(DrawClass::Actor)))
            .add_systems(Update, (apply_ranges.after(fade::fade_cells), share_actor_distance));
        if self.0.size_cull > 0.0 {
            info!("draw distances: objects and items drawn within {} times their bounds' radius (NVF_SIZE_CULL)", self.0.size_cull);
        }
    }
}

/// How far actors are drawn, for Spotter (`nvf_play::perk_marks`): the Actors slider's
/// distance, or as far as their cells at 100%.
fn share_actor_distance(distances: Res<DrawDistances>, mut shared: ResMut<nvf_play::perk_marks::ActorDrawDistance>) {
    if distances.is_changed() {
        shared.set_if_neq(nvf_play::perk_marks::ActorDrawDistance(distances.distance(DrawClass::Actor)));
    }
}

/// The class of a mesh: from its ancestors (an actor's rig, a dropped item, the cell it
/// is in) and the reference's records, kept per reference.
fn classify(e: Entity, r: u32, parents: &Query<&ChildOf>, marks: &Query<(Has<CellEntity>, Has<ActorRig>, Has<DroppedRoot>)>, world: Option<&WorldData>, known: &mut HashMap<u32, DrawClass>) -> DrawClass {
    let mut in_cell = false;
    let mut at = e;
    loop {
        if let Ok((cell, actor, dropped)) = marks.get(at) {
            if actor {
                return DrawClass::Actor;
            }
            if dropped {
                return DrawClass::Item;
            }
            if cell {
                in_cell = true;
                break;
            }
        }
        match parents.get(at) {
            Ok(p) => at = p.parent(),
            Err(_) => break,
        }
    }
    if !in_cell {
        return DrawClass::Unranged;
    }
    *known.entry(r).or_insert_with(|| {
        let Some(w) = world else { return DrawClass::Object };
        let Some(p) = placement(w.lo, w.schema, r) else { return DrawClass::Object };
        if matches!(&p.sig, b"ACHR" | b"ACRE") {
            return DrawClass::Actor;
        }
        match p.base.and_then(|b| base_object(w.lo, w.schema, b)) {
            Some(b) if ITEM_SIGS.contains(&&b.sig) => DrawClass::Item,
            _ => DrawClass::Object,
        }
    })
}

/// Give the meshes of placed references the range of their class's distance: new meshes
/// and those whose range changed (a fade ended) each frame, every one when a slider
/// moved (module notes).
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub(crate) fn apply_ranges(
    mut commands: Commands,
    distances: Res<DrawDistances>,
    world: Option<Res<WorldData>>,
    changed: Query<Entity, (With<RefVisual>, With<MeshMaterial3d<NifMaterial>>, Or<(Added<RefVisual>, Changed<VisibilityRange>, Added<Aabb>)>)>,
    all: Query<Entity, (With<RefVisual>, With<MeshMaterial3d<NifMaterial>>)>,
    mut meshes: Query<(&RefVisual, &mut MeshMaterial3d<NifMaterial>, Option<&VisibilityRange>, Option<&DrawClass>)>,
    parents: Query<&ChildOf>,
    marks: Query<(Has<CellEntity>, Has<ActorRig>, Has<DroppedRoot>)>,
    bounds: Query<(&Aabb, &GlobalTransform)>,
    mut known: Local<HashMap<u32, DrawClass>>,
) {
    let entities: Vec<Entity> = if distances.is_changed() { all.iter().collect() } else { changed.iter().collect() };
    if distances.is_changed() && !distances.is_added() {
        info!(
            "draw distances: objects {:.0}%, actors {:.0}%, items {:.0}% of {:.0} units ({} meshes)",
            distances.objects * 100.0,
            distances.actors * 100.0,
            distances.items * 100.0,
            distances.reach,
            entities.len()
        );
    }
    for e in entities {
        let Ok((r, mut material, range, class)) = meshes.get_mut(e) else { continue };
        let class = match class {
            Some(c) => *c,
            None => {
                let c = classify(e, r.0, &parents, &marks, world.as_deref(), &mut known);
                commands.entity(e).insert(c);
                c
            }
        };
        // The bounds come in PostUpdate (Bevy's `calculate_bounds`): a new mesh is given its
        // class's distance first and its size's once they are there (`Added<Aabb>`).
        let radius = (distances.size_cull > 0.0)
            .then(|| bounds.get(e).ok())
            .flatten()
            .map(|(b, g)| Vec3::from(b.half_extents).length() * g.scale().abs().max_element());
        let want = distances.sized_distance(class, radius);
        match range {
            // Never had a range (cell fades off): one only when it is to be culled.
            None => {
                if let Some(d) = want {
                    commands.entity(e).insert(fade::drawn_within(Some(d), FADE));
                    // Its pipeline is chosen again, with the dither (fade.rs notes).
                    material.set_changed();
                }
            }
            // Fading in or out: the fade's own range until it rests.
            Some(r) if !fade::at_rest(r) => {}
            Some(r) => {
                let target = fade::drawn_within(want, FADE);
                if *r != target {
                    commands.entity(e).insert(target);
                }
            }
        }
    }
}

/// Set a slider's value and act on it now (the settings page).
pub fn set_part(world: &mut World, class: DrawClass, v: f32) {
    let mut d = world.resource_mut::<DrawDistances>();
    match class {
        DrawClass::Object => d.objects = v,
        DrawClass::Actor => d.actors = v,
        DrawClass::Item => d.items = v,
        DrawClass::Unranged => {}
    }
}

/// The sun's shadows reach `v` of `--shadow-distance` from now.
pub fn set_shadows(world: &mut World, v: f32) {
    let full = {
        let mut d = world.resource_mut::<DrawDistances>();
        d.shadows = v;
        d.shadow_full
    };
    if full <= 0.0 {
        return;
    }
    let on = {
        let mut sky = world.resource_mut::<SkyState>();
        sky.shadow_distance = full * v;
        sky.shadows
    };
    // The cascades again, if the sun casts shadows now.
    if on {
        crate::sky::set_shadows(world, true);
    }
}

/// Grass grows out to `v` of `--grass-distance` (and its fade range) from now
/// (`grass.rs` gives its materials the distances).
pub fn set_grass(world: &mut World, v: f32) {
    let (start, range) = {
        let mut d = world.resource_mut::<DrawDistances>();
        d.grass = v;
        d.grass_full
    };
    let mut g = world.resource_mut::<GrassConfig>();
    g.fade_start = start * v;
    g.fade_range = range * v;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn distances() -> DrawDistances {
        DrawDistances { objects: 1.0, actors: 0.5, items: 0.25, shadows: 1.0, grass: 1.0, reach: full_reach(2, 0.0), shadow_full: 8000.0, grass_full: (7000.0, 1000.0), size_cull: 0.0 }
    }

    #[test]
    fn the_full_distance_is_the_loaded_cells_reach() {
        assert!((full_reach(2, 0.0) - 3.0 * CELL_SIZE * std::f32::consts::SQRT_2).abs() < 1e-3);
        // The handheld's 3 x 3 (radius 0, lead 6000): two cells along both axes.
        assert!((full_reach(0, 6000.0) - 2.0 * CELL_SIZE * std::f32::consts::SQRT_2).abs() < 1e-3);
        // A short lead: the lead, its slack and a cell's diagonal.
        assert!((full_reach(0, 1500.0) - (1500.0 + LEAD_SLACK + CELL_SIZE * std::f32::consts::SQRT_2)).abs() < 1e-3);
        let d = distances();
        assert_eq!(d.distance(DrawClass::Object), None, "100%: as far as the cells");
        assert_eq!(d.distance(DrawClass::Actor), Some(d.reach * 0.5));
        assert_eq!(d.distance(DrawClass::Item), Some(d.reach * 0.25));
        assert_eq!(d.distance(DrawClass::Unranged), None);
    }

    #[test]
    fn small_objects_and_items_are_drawn_within_their_size() {
        let mut d = distances();
        // Off: the class's distance alone.
        assert_eq!(d.sized_distance(DrawClass::Object, Some(5.0)), None);
        d.size_cull = 100.0;
        assert_eq!(d.sized_distance(DrawClass::Object, Some(5.0)), Some(500.0));
        // The nearer of the two: an item's slider (a quarter of the reach) beats a crate's size.
        assert_eq!(d.sized_distance(DrawClass::Item, Some(200.0)), Some(d.reach * 0.25));
        // Actors, imposters and meshes not yet bounded keep their class's distance.
        assert_eq!(d.sized_distance(DrawClass::Actor, Some(5.0)), Some(d.reach * 0.5));
        assert_eq!(d.sized_distance(DrawClass::Unranged, Some(5.0)), None);
        assert_eq!(d.sized_distance(DrawClass::Object, None), None);
    }

    #[test]
    fn a_resting_range_ends_at_the_distance_and_full_is_the_open_one() {
        let open = fade::drawn(fade::STEPS);
        assert!(fade::drawn_within(None, FADE) == open);
        let r = fade::drawn_within(Some(4000.0), FADE);
        assert!(fade::at_rest(&r) && fade::at_rest(&open));
        assert!(!fade::at_rest(&fade::drawn(8)));
        assert!(r.is_visible_at_all(3999.0) && r.is_visible_at_all(4300.0));
        assert!(!r.is_visible_at_all(4401.0), "culled past the fade");
    }

    #[test]
    fn the_sliders_set_the_grass_and_the_parts() {
        let mut app = App::new();
        app.insert_resource(distances());
        app.insert_resource(GrassConfig { enabled: true, fade_start: 7000.0, fade_range: 1000.0, light: (1.0, 1.0), generate: Default::default() });
        let w = app.world_mut();
        set_grass(w, 0.5);
        let g = w.resource::<GrassConfig>();
        assert_eq!((g.fade_start, g.fade_range), (3500.0, 500.0));
        set_part(w, DrawClass::Actor, 0.3);
        assert_eq!(w.resource::<DrawDistances>().actors, 0.3);
    }
}
