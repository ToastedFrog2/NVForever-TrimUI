//! Encounter zones on screen (`fnv_game::zones`, docs/spec-notes/encounter-zones.md §5):
//! the drawn actors read their leveled lists at the level the game gives each reference,
//! so the actor drawn is the one the game's rules (factions, packages, carried items)
//! speak of.
//!
//! The game's level of every placed actor is copied into `ActorWorld::levels` whenever it
//! can have changed: a zone was entered, references were fixed at a level (a cell
//! loaded), or the player's level changed. A reference whose level is not fixed yet gets
//! the level its cell's load will fix it at, so an actor drawn just before the game hears
//! of its cell is drawn as it will stay.

use crate::actors::ActorWorld;
use bevy::prelude::*;
use nvf_play::script::ScriptState;

pub struct ZonesPlugin;

impl Plugin for ZonesPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, sync_levels.after(nvf_play::PlaySet::Activation));
    }
}

/// What the copy was made from: fixed references, zones entered, the player's level.
type Key = (usize, usize, u16);

/// The copy is made for the actors of the loaded cells (by where they were placed, and
/// the moved and run-time created ones wherever they are now), not for every actor of the
/// game: walking the whole load order's actors (thousands, every worldspace) at each cell
/// load, zone entered or level gained stopped the TrimUI's frame for seconds (8.6 s
/// measured, docs/trimui.md). A cell's actors are copied when it loads; a zone entered or
/// the player's level changing drops the copy and makes it again; references fixed at a
/// level (a cell loading) copy the loaded cells again. An actor of a cell not loaded is
/// drawn only after its cell loads, by which time it has been copied [design].
#[derive(Default)]
struct Copied {
    key: Option<Key>,
    cells: std::collections::HashSet<fnv_sim::FormId>,
}

fn sync_levels(script: Option<Res<ScriptState>>, sim: Option<Res<nvf_play::sim::SimState>>, actor_world: Option<Res<ActorWorld>>, mut copied: Local<Copied>) {
    let Some(script) = script else { return };
    let Some(rt) = script.rt.as_ref() else { return };
    let (Some(aw), Some(sim)) = (actor_world, sim) else { return };
    let g = &rt.game;
    // The world played is the simulation's: outside a game turn `g.world` is an empty
    // stand-in (`nvf_play::script::with_world`) with no fixed levels.
    let w = &sim.world;
    let player = g.player.level.map_or(w.config().player_level, |l| l.clamp(1, i32::from(u16::MAX)) as u16);
    let key = (w.fixed_levels().len(), w.zone_levels().len(), player);
    let mut fresh = false;
    match copied.key {
        Some(k) if k == key => {}
        // References fixed at a level: the loaded cells again, the rest stays.
        Some(k) if (k.1, k.2) == (key.1, key.2) => copied.cells.clear(),
        _ => {
            copied.cells.clear();
            fresh = true;
        }
    }
    copied.key = Some(key);
    let new: Vec<fnv_sim::FormId> = g.loaded_cells.iter().filter(|c| !copied.cells.contains(*c)).copied().collect();
    if new.is_empty() && !fresh {
        return;
    }
    let is_actor = |r: &fnv_sim::FormId| w.placed.get(*r).is_some_and(|p| matches!(&p.sig, b"ACHR" | b"ACRE"));
    let mut refs: Vec<fnv_sim::FormId> = new.iter().flat_map(|c| w.placed.by_cell.get(c).into_iter().flatten().copied()).filter(is_actor).collect();
    refs.extend(g.moved.iter().copied().filter(is_actor));
    refs.extend(g.created_actors());
    let display = fnv_sim::zones::LevelAt::plain(fnv_actor::resolve::DISPLAY_LEVEL);
    let levels: Vec<(u32, fnv_sim::zones::LevelAt)> = refs.iter().map(|&r| (r.0, sim.leveled_at(g, r))).collect();
    debug!("zones: {} actor reference(s) of {} new cell(s) copied", levels.len(), new.len());
    if let Ok(mut l) = aw.levels.lock() {
        if fresh {
            l.clear();
        }
        for (r, at) in levels {
            if at == display {
                l.remove(&r);
            } else {
                l.insert(r, at);
            }
        }
    }
    copied.cells.extend(new);
}
