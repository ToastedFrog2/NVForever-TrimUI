//! Handheld performance workbench. No changes until the player explicitly makes one.
//! Render-only exclusions never disable simulation references or enter game saves.
use bevy::camera::primitives::Aabb;
use bevy::camera::visibility::RenderLayers;
use bevy::input::InputSystems;
use bevy::prelude::*;
use nvf_play::settings::{ChoiceList, Setting, Settings};
use nvf_play::{PlayState, RefVisual};
use std::collections::{BTreeMap, BTreeSet};
#[path = "trimui_admin_model.rs"]
mod model;

pub struct AdminPlugin;

#[derive(Resource, Default)]
struct Admin {
    open: bool,
    page: usize,
    row: usize,
    root: Option<Entity>,
    body: Option<Entity>,
    status: Option<Entity>,
    note: Option<Entity>,
    filters: u16,
    far: f32,
    hidden: BTreeSet<u32>,
    undo: Vec<u32>,
    aimed: Option<u32>,
    baseline: Vec<(Setting, f32, Option<(ChoiceList, String)>)>,
    baseline_cull: f32,
    captured: Vec<(Entity, Gamepad)>,
    repeat_at: f64,
    repeat_button: Option<GamepadButton>,
    quit_held: f32,
    chord: bool,
    next_draw: f64,
    fps_time: f32,
    fps_frames: u32,
    fps: f32,
    message: String,
    had_filters: bool,
    dev_before: Option<(bool, bool, bool, bool, f32)>,
    bookmark: Option<nvf_play::Travel>,
    bench: Option<(f32, Vec<f32>)>,
    last_bench: String,
}

#[derive(Component)]
struct AdminHidden(Option<RenderLayers>);

const LAND: u16 = 1;
const SKY: u16 = 2;
const ACTORS: u16 = 4;
const MODELS: u16 = 8;
const WATER: u16 = 16;
const GRASS: u16 = 32;
const CLOUDS: u16 = 64;
const TREES: u16 = 128;
const PAGE_ROWS: usize = 8;
const FLAGS: [(&str, u16); 8] = [
    ("Placed objects / buildings", MODELS), ("People and creatures", ACTORS),
    ("Terrain", LAND), ("Sky", SKY), ("Clouds", CLOUDS),
    ("Water", WATER), ("Grass", GRASS), ("Trees / billboards", TREES),
];

impl Plugin for AdminPlugin {
    fn build(&self, app: &mut App) {
        let read = |path: std::path::PathBuf| match std::fs::read_to_string(&path) {
            Ok(text) => Some(text),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => { warn!("admin: {} unreadable: {e}; developer menu disabled",path.display()); Some(String::new()) }
        };
        let packaged = std::env::current_exe().ok().and_then(|p| p.parent().map(|p|p.join("dev-menu.ini"))).and_then(read);
        let user = std::env::var_os("NVFOREVER_HOME").filter(|p| !p.is_empty())
            .and_then(|p|read(std::path::PathBuf::from(p).join("dev-menu.ini")));
        if !model::dev_menu_enabled(packaged.as_deref(),user.as_deref()) {
            info!("admin: developer menu disabled by dev-menu.ini");
            return;
        }
        info!("admin: developer menu enabled by dev-menu.ini");
        app.init_resource::<Admin>()
            .add_systems(PreUpdate, input.after(InputSystems)
                .before(nvf_play::pad::menu_keys).before(nvf_play::pad::update_stick_buttons))
            .add_systems(Update, draw)
            .add_systems(PostUpdate, filter_meshes
                .after(bevy::transform::TransformSystems::Propagate)
                .before(bevy::camera::visibility::VisibilitySystems::CheckVisibility))
            .add_systems(Last, restore_pads);
    }
}

fn allowed(s: &Setting) -> bool {
    // The handheld has one physical mode. Account/report actions do not belong here.
    !matches!(s.key, "resolution" | "send-reports" | "reports-delete")
}

fn groups(world: &World) -> Vec<&'static str> {
    let mut groups = Vec::new();
    if let Some(settings) = world.get_resource::<Settings>() {
        for s in settings.list().iter().filter(|s| allowed(s)) {
            if !groups.contains(&s.group) { groups.push(s.group); }
        }
    }
    groups
}

#[derive(Clone)]
enum Action {
    HideAimed, Undo, RestoreObjects, Save, Load, Reset,
    Flag(u16), Far, Cull, Reference(u32), Setting(Setting, Option<ChoiceList>),
    Developer(&'static str), Benchmark, Screenshot, Report,
}

struct Row {
    label: String,
    value: String,
    note: String,
    action: Action,
}

fn reference_name(world: &World, id: u32) -> String {
    world.get_resource::<nvf_play::sim::SimState>()
        .and_then(|s| s.base_of(id).and_then(|b| s.forms.get(b)))
        .and_then(|b| b.name.as_ref().or(b.edid.as_ref())).cloned()
        .unwrap_or_else(|| format!("Reference {id:08X}"))
}

fn scene_refs(world: &mut World) -> Vec<(u32, f32)> {
    let eye = eye(world).map(|(_, p, _)| p).unwrap_or(Vec3::ZERO);
    let mut refs = BTreeMap::<u32, f32>::new();
    let mut q = world.query_filtered::<(&RefVisual, &GlobalTransform), With<Mesh3d>>();
    for (r, at) in q.iter(world) {
        if r.0 == 0x14 { continue; }
        let d = at.translation().distance(eye);
        refs.entry(r.0).and_modify(|old| *old = old.min(d)).or_insert(d);
    }
    let mut refs: Vec<_> = refs.into_iter().collect();
    refs.sort_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)));
    refs
}

fn rows(world: &mut World, admin: &Admin) -> Vec<Row> {
    let row = |label: &str, value: String, note: &str, action| Row { label: label.into(), value, note: note.into(), action };
    match admin.page {
        0 => vec![
            row("Hide aimed object", admin.aimed.map(|r| reference_name(world, r)).unwrap_or_else(|| "No object aimed at".into()),
                "Aim at an object, then press Menu. A hides its render meshes. Collision and quest state stay intact.", Action::HideAimed),
            row("Undo last object hide", admin.undo.len().to_string(), "Restore the last individually hidden object. X also undoes from any page.", Action::Undo),
            row("Restore all individual objects", admin.hidden.len().to_string(), "Category toggles and distance limits stay as chosen.", Action::RestoreObjects),
            row("Save tuning profile", "A".into(), "Save these experiments separately from game saves. It is never applied automatically.", Action::Save),
            row("Load tuning profile", "A".into(), "Apply your explicitly saved performance profile to this session.", Action::Load),
            row("Restore launch tuning", "A / Y".into(), "Restore render settings, AI, noclip and simulation speed. Inventory changes, game clock and travel are not undone.", Action::Reset),
            row("Benchmark 30 seconds", admin.last_bench.clone(), "Closes the panel and measures 30 seconds of real gameplay. Stand still for comparisons, or follow the same route.", Action::Benchmark),
            row("Capture clean screenshot", "A".into(), "Closes the panel and saves a timestamped screenshot in your game's screenshots folder.", Action::Screenshot),
            row("Write performance report", "A".into(), "Write settings, memory, CPU, GPU status and benchmark results into the game home's admin-reports folder.", Action::Report),
        ],
        1 => FLAGS.iter().map(|(label, bit)| row(label, if admin.filters & bit == 0 { "Shown" } else { "Hidden" }.into(),
            "Render-only switch. The world keeps running so you can compare FPS. Close the panel for a clean gameplay measurement.", Action::Flag(*bit))).chain([
            row("Placed model distance", if admin.far == 0.0 { "Unlimited".into() } else { format!("{:.0} units", admin.far) },
                "Left/right: 2,000-unit steps. 0 means no extra limit. Terrain and sky keep their existing reach.", Action::Far),
            row("Small object size culling", world.get_resource::<crate::draw_distance::DrawDistances>().map_or("Unavailable".into(), |d| if d.size_cull == 0.0 { "Off".into() } else { format!("{:.0} radii", d.size_cull) }),
                "Off or 25-200 times each object's radius. Smaller values hide small details sooner; large buildings remain longer.", Action::Cull),
        ]).collect(),
        2 => scene_refs(world).into_iter().map(|(id, distance)| row(
            &format!("{} [{id:08X}]", reference_name(world, id)),
            format!("{} / {:.0}u", if admin.hidden.contains(&id) { "Hidden" } else { "Shown" }, distance),
            "Loaded references, nearest first. A hides/restores every mesh of this reference. This does not delete its collision or simulation.", Action::Reference(id))).collect(),
        3 => developer_rows(world),
        page => {
            let groups = groups(world);
            let Some(group) = groups.get(page - 4) else { return Vec::new() };
            let Some(settings) = world.get_resource::<Settings>() else { return Vec::new() };
            settings.list().iter().enumerate().filter(|(_, s)| allowed(s) && &s.group == group).map(|(i, s)| {
                let choices = settings.choices(i);
                let value = if let Some(c) = choices { (c.chosen)(world) }
                    else if let Some(slider) = s.slider { slider.text((slider.get)(world)) }
                    else { if (s.get)(world) { "On" } else { "Off" }.into() };
                row(s.label, value, s.note, Action::Setting(*s, choices))
            }).collect()
        }
    }
}

fn capture_baseline(world: &World, admin: &mut Admin) {
    if !admin.baseline.is_empty() { return; }
    if let Some(settings) = world.get_resource::<Settings>() {
        admin.baseline = settings.list().iter().enumerate().filter(|(_, s)| allowed(s)).map(|(i, s)|
            (*s, s.value(world), settings.choices(i).map(|c| (c, (c.save)(world))))).collect();
    }
    admin.baseline_cull = world.get_resource::<crate::draw_distance::DrawDistances>().map_or(0.0, |d| d.size_cull);
    let (ai, scripts, detection) = game_toggles(world);
    admin.dev_before = Some((world.get_resource::<nvf_play::player::NoClip>().is_some_and(|n| n.0), ai, scripts, detection,
        world.resource::<Time<Virtual>>().relative_speed()));
}

fn reset(world: &mut World, admin: &mut Admin) {
    for (s, value, choice) in &admin.baseline {
        if let Some((c, value)) = choice { (c.load)(world, value); }
        else { s.put(world, *value); }
    }
    if let Some(mut d) = world.get_resource_mut::<crate::draw_distance::DrawDistances>() { d.size_cull = admin.baseline_cull; }
    admin.filters = 0;
    admin.far = 0.0;
    admin.hidden.clear();
    admin.undo.clear();
    if let Some((noclip, ai, scripts, detection, speed)) = admin.dev_before {
        if let Some(mut n) = world.get_resource_mut::<nvf_play::player::NoClip>() { n.0 = noclip; }
        if let Some(mut s) = world.get_resource_mut::<nvf_play::script::ScriptState>() {
            if let Some(rt) = s.rt.as_mut() { rt.game.toggles.actors_ai_off = ai; rt.game.toggles.scripts_off = scripts; rt.game.toggles.detection_off = detection; }
        }
        world.resource_mut::<Time<Virtual>>().set_relative_speed(speed);
    }
    if world.get_resource::<nvf_play::free_cam::FreeCam>().is_some_and(|f| f.on) { console_command(world, "tfc"); }
    admin.message = "Launch settings restored.".into();
    info!("admin: launch settings restored");
}

fn profile_path() -> Option<std::path::PathBuf> {
    std::env::var_os("NVFOREVER_HOME").map(|p| std::path::PathBuf::from(p).join("admin-profile.json"))
}

fn game_toggles(world: &World) -> (bool, bool, bool) {
    world.get_resource::<nvf_play::script::ScriptState>().and_then(|s| s.rt.as_ref())
        .map(|rt| (rt.game.toggles.actors_ai_off, rt.game.toggles.scripts_off, rt.game.toggles.detection_off)).unwrap_or_default()
}

fn developer_rows(world: &World) -> Vec<Row> {
    let row = |label: &str, value: String, note: &str, key| Row { label: label.into(), value, note: note.into(), action: Action::Developer(key) };
    let yes = |v| if v { "On" } else { "Off" }.to_string();
    let (ai, scripts, detection) = game_toggles(world);
    vec![
        row("God mode", yes(nvf_play::combat::god_mode()), "Protect the player while testing. Cheats and travel affect gameplay if you later save.", "god"),
        row("Noclip / collision bypass", yes(world.get_resource::<nvf_play::player::NoClip>().is_some_and(|n| n.0)), "Fly the player through geometry. Switch it off again above solid ground.", "noclip"),
        row("Actor AI", yes(!ai), "Stop NPC decisions to measure their CPU cost. This changes gameplay while enabled.", "ai"),
        row("Per-frame scripts", yes(!scripts), "Skip GameMode/MenuMode script blocks to measure script cost. Event scripts still run.", "scripts"),
        row("NPC player detection", yes(!detection), "Prevent NPCs detecting the player during inspection.", "detection"),
        row("Free camera", yes(world.get_resource::<nvf_play::free_cam::FreeCam>().is_some_and(|f| f.on)), "Closes panel. Left stick moves, right stick looks, L2/R2 move down/up, A boosts. Reopen Menu to return to player.", "freecam"),
        row("Simulation speed", format!("{:.2}x", world.resource::<Time<Virtual>>().relative_speed()), "Left/right: freeze, quarter, half, normal, 1.5x or 2x virtual time. Real-time systems keep their own clocks.", "speed"),
        row("Remember this position", "A".into(), "Store a session-only position bookmark before taking a test route.", "bookmark"),
        row("Return to position bookmark", "A".into(), "Travel back through the normal loading/physics system. No game save is created.", "return"),
        row("Game clock: next hour", "A".into(), "Advance the game clock by one hour. A fixed sky may retain its own appearance.", "hour"),
        row("Game clock: noon", "A".into(), "Set GameHour to 12; this changes the game clock, not the rendering speed.", "noon"),
        row("Game clock: stop / normal", "A".into(), "Toggle TimeScale between 0 and the usual 30. NPC physics continues.", "clock"),
        row("Restore player health", "A".into(), "Heal the player for combat testing. Changes gameplay state.", "heal"),
        row("Give player 1,000 caps", "A".into(), "Add test currency to the player's inventory. Changes gameplay state.", "caps"),
        row("Release unused heap pages", "A".into(), "Ask glibc to return already-free heap pages to Linux. May briefly stall; does not unload live assets.", "trim"),
    ]
}

fn console_command(world: &mut World, line: &str) {
    if let Some(mut s) = world.get_resource_mut::<nvf_play::script::ScriptState>() {
        s.console.push(nvf_play::console::ConsoleCall { line: line.into(), selected: None, active_quest: None });
        info!("admin: console command {line}");
    }
}

fn developer_action(world: &mut World, admin: &mut Admin, key: &str, direction: i32) {
    match key {
        "god" => nvf_play::combat::set_god_mode(!nvf_play::combat::god_mode()),
        "noclip" => if let Some(mut n) = world.get_resource_mut::<nvf_play::player::NoClip>() { n.0 = !n.0; },
        "ai" | "scripts" | "detection" => if let Some(mut s) = world.get_resource_mut::<nvf_play::script::ScriptState>() {
            if let Some(rt) = s.rt.as_mut() {
                match key { "ai" => rt.game.toggles.actors_ai_off = !rt.game.toggles.actors_ai_off,
                    "scripts" => rt.game.toggles.scripts_off = !rt.game.toggles.scripts_off,
                    _ => rt.game.toggles.detection_off = !rt.game.toggles.detection_off }
            }
        },
        "freecam" => { console_command(world, "tfc"); admin.open = false; }
        "speed" => {
            let speeds = [0.0, 0.25, 0.5, 1.0, 1.5, 2.0];
            let current = world.resource::<Time<Virtual>>().relative_speed();
            let i = speeds.iter().position(|v| *v == current).unwrap_or(3);
            world.resource_mut::<Time<Virtual>>().set_relative_speed(speeds[(i as i32 + direction).rem_euclid(speeds.len() as i32) as usize]);
        }
        "bookmark" => {
            if let (Some(location), Some(player)) = (world.get_resource::<nvf_play::Location>(), world.get_resource::<nvf_play::player::Player>()) {
                admin.bookmark = Some(nvf_play::Travel { world: Some(location.world), interior: location.interior, feet: player.feet, heading_deg: player.heading.to_degrees() });
                admin.message = "Position bookmarked for this session.".into();
            }
        }
        "return" => if let Some(place) = admin.bookmark { world.write_message(nvf_play::loading::TravelRequest(place)); admin.open = false; }
            else { admin.message = "Remember a position first.".into(); },
        "hour" => {
            let hour = world.get_resource::<nvf_play::sim::SimState>().and_then(|s| s.world.calendar().ok()).map_or(12.0, |c| (c.hour + 1.0) % 24.0);
            console_command(world, &format!("set GameHour to {hour}"));
        }
        "noon" => console_command(world, "set GameHour to 12"),
        "clock" => {
            let scale = world.get_resource::<nvf_play::sim::SimState>().and_then(|s| s.world.time_scale()).unwrap_or(30.0);
            console_command(world, if scale == 0.0 { "set TimeScale to 30" } else { "set TimeScale to 0" });
        }
        "heal" => console_command(world, "player.ResetHealth"),
        "caps" => console_command(world, "player.additem 0000000F 1000"),
        "trim" => { unsafe { libc::malloc_trim(0); } admin.message = "Unused heap pages released.".into(); }
        _ => {}
    }
    info!("admin: developer action {key}");
}

fn timestamp() -> u64 { std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs() }

fn write_report(world: &World, admin: &Admin) -> Result<(), String> {
    let dir = profile_path().ok_or("No game settings directory")?.parent().unwrap().join("admin-reports");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let values: BTreeMap<_,_> = world.get_resource::<Settings>().map(|s| s.list().iter().filter(|s| allowed(s)).map(|s| (s.key,s.value(world))).collect()).unwrap_or_default();
    let read = |path| std::fs::read_to_string(path).unwrap_or_default();
    let report = serde_json::json!({"version":1,"benchmark":admin.last_bench,"filters":admin.filters,"hidden":admin.hidden,
        "model_distance":admin.far,"settings":values,"process_memory":read("/proc/self/status"),
        "system_memory":read("/proc/meminfo"),"gpu":read("/sys/kernel/debug/pvr/status"),
        "cpu_khz":read_number("/sys/devices/system/cpu/cpufreq/policy0/scaling_cur_freq"),
        "temperature_mC":read_number("/sys/class/thermal/thermal_zone0/temp")});
    std::fs::write(dir.join(format!("report-{}.json",timestamp())),serde_json::to_vec_pretty(&report).map_err(|e|e.to_string())?).map_err(|e|e.to_string())
}

fn save_profile(world: &World, admin: &Admin) -> Result<(), String> {
    let path = profile_path().ok_or("No game settings directory")?;
    fnv_install::guard_write(&path, None).map_err(|e| e.to_string())?;
    let mut numeric = serde_json::Map::new();
    let mut choices = serde_json::Map::new();
    if let Some(settings) = world.get_resource::<Settings>() {
        for (i, s) in settings.list().iter().enumerate().filter(|(_, s)| allowed(s)) {
            if let Some(c) = settings.choices(i) { choices.insert(s.key.into(), (c.save)(world).into()); }
            else { numeric.insert(s.key.into(), serde_json::json!(s.value(world))); }
        }
    }
    let cull = world.get_resource::<crate::draw_distance::DrawDistances>().map_or(0.0, |d| d.size_cull);
    let json = serde_json::json!({"version":1,"filters":admin.filters,"model_distance":admin.far,"size_cull":cull,
        "hidden_references":admin.hidden,"settings":numeric,"choices":choices});
    let staging = path.with_extension("json.tmp");
    std::fs::write(&staging, serde_json::to_vec_pretty(&json).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    std::fs::rename(&staging, &path).map_err(|e| { let _ = std::fs::remove_file(&staging); e.to_string() })
}

fn load_profile(world: &mut World, admin: &mut Admin) -> Result<(), String> {
    let path = profile_path().ok_or("No game settings directory")?;
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let json: serde_json::Value = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    if json["version"].as_u64() != Some(1) { return Err("Unrecognized profile version".into()); }
    let settings: Vec<_> = world.get_resource::<Settings>().map(|settings| settings.list().iter().enumerate()
        .filter(|(_, s)| allowed(s)).map(|(i, s)| (*s, settings.choices(i))).collect()).unwrap_or_default();
    for (s, choice) in settings {
        if let Some(c) = choice {
            if let Some(value) = json["choices"][s.key].as_str() { (c.load)(world, value); }
        } else if let Some(value) = json["settings"][s.key].as_f64().filter(|v| v.is_finite()) { s.put(world, value as f32); }
    }
    admin.filters = json["filters"].as_u64().unwrap_or(0) as u16 & 255;
    admin.far = json["model_distance"].as_f64().filter(|v| v.is_finite()).unwrap_or(0.0).clamp(0.0, 40000.0) as f32;
    admin.hidden = json["hidden_references"].as_array().map(|values| values.iter().filter_map(|v| v.as_u64()
        .filter(|id| *id <= u32::MAX as u64 && *id != 0x14).map(|v| v as u32)).collect()).unwrap_or_default();
    admin.undo.clear();
    if let Some(mut d) = world.get_resource_mut::<crate::draw_distance::DrawDistances>() {
        d.size_cull = json["size_cull"].as_f64().filter(|v| v.is_finite()).unwrap_or(0.0).clamp(0.0, 200.0) as f32;
    }
    Ok(())
}

fn hide_reference(admin: &mut Admin, id: u32) {
    model::toggle_hidden(&mut admin.hidden, &mut admin.undo, id);
    admin.message = format!("Object {id:08X}: {}", if admin.hidden.contains(&id) { "hidden visually" } else { "restored" });
    info!("admin: {}", admin.message);
}

fn act(world: &mut World, admin: &mut Admin, action: Action, direction: i32) {
    match action {
        Action::HideAimed => if let Some(id) = admin.aimed { hide_reference(admin, id); },
        Action::Reference(id) => hide_reference(admin, id),
        Action::Undo => if let Some(id) = admin.undo.pop() { admin.hidden.remove(&id); admin.message = format!("Restored {id:08X}"); },
        Action::RestoreObjects => { admin.hidden.clear(); admin.undo.clear(); admin.message = "All individually hidden objects restored.".into(); }
        Action::Flag(bit) => { admin.filters ^= bit; info!("admin: category mask {}", admin.filters); }
        Action::Far => { admin.far = (admin.far + direction as f32 * 2000.0).clamp(0.0, 40000.0); }
        Action::Cull => if let Some(mut d) = world.get_resource_mut::<crate::draw_distance::DrawDistances>() {
            let choices = [0.0, 200.0, 150.0, 100.0, 80.0, 50.0, 25.0];
            let i = choices.iter().position(|v| *v == d.size_cull).unwrap_or(0);
            d.size_cull = choices[(i as i32 + direction).rem_euclid(choices.len() as i32) as usize];
        },
        Action::Reset => reset(world, admin),
        Action::Developer(key) => developer_action(world, admin, key, direction),
        Action::Benchmark => { admin.bench = Some((0.0, Vec::new())); admin.open = false; admin.message = "Benchmark running for 30 seconds.".into(); info!("admin: 30 second benchmark started"); }
        Action::Screenshot => {
            if let Some(home) = std::env::var_os("NVFOREVER_HOME") {
                let dir = std::path::PathBuf::from(home).join("screenshots");
                match std::fs::create_dir_all(&dir) {
                    Ok(()) => { let path = dir.join(format!("admin-{}.png", timestamp()));
                        world.spawn(bevy::render::view::screenshot::Screenshot::primary_window())
                            .observe(bevy::render::view::screenshot::save_to_disk(path)); admin.open = false; }
                    Err(e) => admin.message = e.to_string(),
                }
            }
        }
        Action::Report => { admin.message = match write_report(world, admin) { Ok(()) => "Performance report saved in admin-reports.".into(), Err(e) => e }; }
        Action::Save => { admin.message = match save_profile(world, admin) { Ok(()) => "Tuning profile saved separately. Load it explicitly when wanted.".into(), Err(e) => format!("Save: {e}") }; }
        Action::Load => { admin.message = match load_profile(world, admin) { Ok(()) => "Tuning profile loaded.".into(), Err(e) => format!("Load: {e}") }; }
        Action::Setting(s, choice) => {
            if let Some(c) = choice {
                let names = (c.names)(world);
                if !names.is_empty() {
                    let i = names.iter().position(|n| *n == (c.chosen)(world)).unwrap_or(0);
                    let next = (i as i32 + direction).rem_euclid(names.len() as i32) as usize;
                    (c.pick)(world, &names[next]);
                }
            } else if let Some(slider) = s.slider {
                s.put(world, (slider.get)(world) + direction as f32 * slider.step);
            } else { let on = (s.get)(world); (s.set)(world, !on); }
            info!("admin: changed {} (session only)", s.key);
        }
    }
    admin.next_draw = 0.0;
}

fn eye(world: &mut World) -> Option<(Entity, Vec3, Vec3)> {
    let mut q = world.query_filtered::<(Entity, &GlobalTransform), With<nvf_play::PlayerCamera>>();
    q.iter(world).next().map(|(e, t)| (e, t.translation(), *t.forward()))
}

// Intersection in model space, with the ray parameter still measured in world units.
fn ray_box(origin: Vec3, direction: Vec3, min: Vec3, max: Vec3) -> Option<f32> {
    model::ray_box(origin.to_array(), direction.to_array(), min.to_array(), max.to_array())
}

fn pick_reference(world: &mut World, admin: &Admin) -> Option<u32> {
    let (_, origin, direction) = eye(world)?;
    let mut q = world.query_filtered::<(&RefVisual, &Aabb, &GlobalTransform, Option<&ViewVisibility>), With<Mesh3d>>();
    q.iter(world).filter(|(r, _, _, visible)| r.0 != 0x14 && !admin.hidden.contains(&r.0) && visible.is_none_or(|v| v.get()))
        .filter_map(|(r, bounds, at, _)| {
            let inverse = at.affine().inverse();
            if !inverse.is_finite() { return None; }
            let hit = ray_box(inverse.transform_point3(origin), inverse.transform_vector3(direction),
                (bounds.center - bounds.half_extents).into(), (bounds.center + bounds.half_extents).into())?;
            Some((r.0, hit))
        }).min_by(|a, b| a.1.total_cmp(&b.1)).map(|(id, _)| id)
}

/// Consume physical pad input before the game maps buttons into keyboard/menu actions.
/// Real components are put back in Last, preserving held axes and next-frame releases.
fn input(world: &mut World) {
    let mut admin = world.remove_resource::<Admin>().unwrap_or_default();
    let time = world.resource::<Time<Real>>();
    let (now, dt) = (time.elapsed_secs_f64(), time.delta_secs());
    let mut q = world.query::<&Gamepad>();
    let pressed: Vec<_> = q.iter(world).flat_map(|g| g.get_pressed().copied()).collect();
    let just: Vec<_> = q.iter(world).flat_map(|g| g.get_just_pressed().copied()).collect();
    let released_menu = q.iter(world).any(|g| g.just_released(GamepadButton::Mode));
    let both = pressed.contains(&GamepadButton::Mode) && pressed.contains(&GamepadButton::Start);
    if both { admin.chord = true; admin.quit_held += dt; } else { admin.quit_held = 0.0; }
    if admin.quit_held >= 1.0 { world.write_message(AppExit::Success); }
    let free = world.get_resource::<nvf_play::free_cam::FreeCam>().is_some_and(|f| f.holds_world());
    let can_open = world.get_resource::<State<PlayState>>().is_some_and(|s| *s.get() == PlayState::Playing || (free && *s.get() == PlayState::Menu))
        && !world.get_resource::<nvf_play::pause::Paused>().is_some_and(|p| p.0)
        && !world.get_resource::<nvf_play::main_menu::MainMenu>().is_some_and(|m| m.open);
    let was_open = admin.open;
    if released_menu {
        if !admin.chord && (admin.open || can_open) {
            admin.open = !admin.open;
            if admin.open {
                capture_baseline(world, &mut admin);
                admin.aimed = pick_reference(world, &admin);
                admin.message = "Live experiments. Close the panel to measure without its drawing overhead.".into();
                admin.next_draw = 0.0;
            }
            info!("admin: panel {}", if admin.open { "opened" } else { "closed" });
        }
        admin.chord = false;
    }
    if admin.open && !can_open { admin.open = false; }
    if admin.open {
        let page_count = 4 + groups(world).len();
        if just.contains(&GamepadButton::LeftTrigger) { admin.page = (admin.page + page_count - 1) % page_count; admin.row = 0; admin.next_draw = 0.0; }
        if just.contains(&GamepadButton::RightTrigger) { admin.page = (admin.page + 1) % page_count; admin.row = 0; admin.next_draw = 0.0; }
        if just.contains(&GamepadButton::East) { admin.open = false; }
        let button = [GamepadButton::DPadUp, GamepadButton::DPadDown, GamepadButton::DPadLeft, GamepadButton::DPadRight]
            .into_iter().find(|b| pressed.contains(b));
        let navigation = button.filter(|b| just.contains(b) || (admin.repeat_button == Some(*b) && now >= admin.repeat_at));
        if let Some(b) = navigation {
            let initial = just.contains(&b) || admin.repeat_button != Some(b);
            admin.repeat_at = now + if initial { 0.4 } else { 0.14 };
            let rows = rows(world, &admin);
            if !rows.is_empty() {
                match b {
                    GamepadButton::DPadUp => admin.row = admin.row.saturating_sub(1),
                    GamepadButton::DPadDown => admin.row = (admin.row + 1).min(rows.len() - 1),
                    GamepadButton::DPadLeft | GamepadButton::DPadRight => if let Some(row) = rows.get(admin.row) {
                        if matches!(row.action, Action::Setting(_, _) | Action::Far | Action::Cull | Action::Flag(_) | Action::Developer("speed")) {
                            act(world, &mut admin, row.action.clone(), if b == GamepadButton::DPadLeft { -1 } else { 1 });
                        }
                    },
                    _ => {}
                }
            }
            admin.next_draw = 0.0;
        }
        admin.repeat_button = button;
        if just.contains(&GamepadButton::South) {
            if let Some(row) = rows(world, &admin).get(admin.row) { act(world, &mut admin, row.action.clone(), 1); }
        }
        if just.contains(&GamepadButton::West) { act(world, &mut admin, Action::Undo, 1); }
        if just.contains(&GamepadButton::North) { reset(world, &mut admin); admin.next_draw = 0.0; }
    }
    if was_open || admin.open {
        let mut q = world.query::<(Entity, &mut Gamepad)>();
        admin.captured = q.iter_mut(world).map(|(e, mut g)| (e, std::mem::take(&mut *g))).collect();
    }
    world.insert_resource(admin);
}

fn restore_pads(world: &mut World) {
    let captured = std::mem::take(&mut world.resource_mut::<Admin>().captured);
    for (e, pad) in captured { if let Some(mut current) = world.get_mut::<Gamepad>(e) { *current = pad; } }
}

fn draw(world: &mut World) {
    let mut admin = world.remove_resource::<Admin>().unwrap_or_default();
    let time = world.resource::<Time<Real>>();
    let (now, dt) = (time.elapsed_secs_f64(), time.delta_secs());
    if let Some((elapsed, samples)) = admin.bench.as_mut() {
        let playing = world.get_resource::<State<PlayState>>().is_some_and(|s| *s.get() == PlayState::Playing);
        if playing && dt > 0.0 { *elapsed += dt; samples.push(dt); }
        if *elapsed >= 30.0 {
            let fps = samples.len() as f32 / *elapsed;
            samples.sort_unstable_by(f32::total_cmp);
            let p95 = samples[((samples.len() as f32 * 0.95) as usize).min(samples.len()-1)] * 1000.0;
            admin.last_bench = format!("{fps:.1} FPS / p95 {p95:.1} ms");
            admin.message = format!("Benchmark: {}",admin.last_bench);
            info!("admin: {}",admin.message);
            admin.bench = None;
        }
    }
    if !admin.open {
        if let Some(root) = admin.root.take() { world.despawn(root); }
        admin.body = None; admin.status = None; admin.note = None;
        admin.fps_time = 0.0; admin.fps_frames = 0;
        world.insert_resource(admin); return;
    }
    admin.fps_time += dt; admin.fps_frames += 1;
    if admin.fps_time >= 1.0 { admin.fps = admin.fps_frames as f32 / admin.fps_time; admin.fps_time = 0.0; admin.fps_frames = 0; }
    if admin.root.is_none() {
        let root = world.spawn((Node { position_type: PositionType::Absolute, top: Val::Px(30.0), right: Val::Px(16.0),
            width: Val::Px(690.0), height: Val::Px(660.0), padding: UiRect::all(Val::Px(20.0)), flex_direction: FlexDirection::Column,
            row_gap: Val::Px(12.0), ..default() }, BackgroundColor(Color::srgba(0.02, 0.025, 0.025, 0.96)), GlobalZIndex(250))).id();
        let label = |world: &mut World, text: &str, size: f32, color: Color| world.spawn((Text::new(text),
            TextFont { font_size: FontSize::Px(size), ..default() }, TextColor(color), ChildOf(root))).id();
        label(world, "NVFOREVER  /  ADMIN & PERFORMANCE", 25.0, Color::srgb(1.0, 0.75, 0.3));
        admin.status = Some(label(world, "", 18.0, Color::srgb(0.65, 0.9, 0.8)));
        admin.body = Some(label(world, "", 20.0, Color::srgb(1.0, 0.89, 0.65)));
        admin.note = Some(label(world, "", 17.0, Color::srgb(0.8, 0.82, 0.82)));
        label(world, "LB/RB  tabs     D-pad  select / change     A  apply\nB / Menu  close     X  undo object     Y  reset tuning", 17.0, Color::srgb(0.6, 0.72, 0.68));
        admin.root = Some(root); admin.next_draw = 0.0;
    }
    if now >= admin.next_draw {
        admin.next_draw = now + 0.5;
        let groups = groups(world);
        let name = match admin.page { 0 => "Workbench", 1 => "Visibility & detail", 2 => "Loaded object browser", 3 => "Developer tools", n => groups.get(n - 4).copied().unwrap_or("Settings") };
        let rows = rows(world, &admin);
        admin.row = admin.row.min(rows.len().saturating_sub(1));
        let start = admin.row / PAGE_ROWS * PAGE_ROWS;
        let mut body = format!("<  {} / {}   {}  >\n\n", admin.page + 1, groups.len() + 4, name);
        for (i, row) in rows.iter().enumerate().skip(start).take(PAGE_ROWS) {
            let label: String = row.label.chars().take(34).collect();
            let value: String = row.value.chars().take(25).collect();
            body.push_str(&format!("{} {}: {}\n", if i == admin.row { ">" } else { " " }, label, value));
        }
        if rows.is_empty() { body.push_str("No rows available in this scene.\n"); }
        if let Some(e) = admin.body {
            if let Some(mut t) = world.get_mut::<Text>(e) { t.set_if_neq(Text::new(body)); }
            if let Some(mut f) = world.get_mut::<TextFont>(e) {
                if f.font_size != FontSize::Px(18.0) { f.font_size = FontSize::Px(18.0); }
            }
        }
        let cpu = read_number("/sys/devices/system/cpu/cpufreq/policy0/scaling_cur_freq") / 1000.0;
        let temp = read_number("/sys/class/thermal/thermal_zone0/temp") / 1000.0;
        let mem = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
        let field = |name: &str| mem.lines().find_map(|l| l.strip_prefix(name)?.split_whitespace().next()?.parse::<f32>().ok()).unwrap_or(0.0) / 1024.0;
        if let Some(e) = admin.status { if let Some(mut t) = world.get_mut::<Text>(e) {
            t.set_if_neq(Text::new(format!("{:.1} FPS    CPU {:.0} MHz / {:.1} C\nRAM {:.0} MB   Swap {:.0} MB   Hidden objects {}", admin.fps, cpu, temp, field("VmRSS:"), field("VmSwap:"), admin.hidden.len())));
        } }
        let note = rows.get(admin.row).map_or("", |r| r.note.as_str());
        if let Some(e) = admin.note { if let Some(mut t) = world.get_mut::<Text>(e) {
            t.set_if_neq(Text::new(format!("{}/{}  {}\n{}", admin.row + 1, rows.len(), note.chars().take(190).collect::<String>(), admin.message.chars().take(130).collect::<String>())));
        } }
    }
    world.insert_resource(admin);
}

fn read_number(path: &str) -> f32 { std::fs::read_to_string(path).ok().and_then(|s| s.trim().parse().ok()).unwrap_or(0.0) }

#[allow(clippy::type_complexity)]
fn filter_meshes(
    mut commands: Commands, mut admin: ResMut<Admin>,
    cameras: Query<&GlobalTransform, With<nvf_play::PlayerCamera>>,
    meshes: Query<(Entity, &GlobalTransform, Option<&RefVisual>, Has<crate::terrain::LandMesh>,
        Has<MeshMaterial3d<crate::sky::SkyMaterial>>, Has<crate::sky::CloudLayer>,
        Has<bevy::mesh::skinning::SkinnedMesh>, Has<MeshMaterial3d<crate::water::WaterMaterial>>,
        Has<crate::grass::GrassChunk>, Has<crate::world::TreeBillboard>,
        Has<MeshMaterial3d<crate::tree::LeafMaterial>>, Has<MeshMaterial3d<crate::tree::BarkMaterial>>,
        Option<&crate::draw_distance::DrawClass>,
        Option<&RenderLayers>, Option<&AdminHidden>), With<Mesh3d>>,
) {
    let active = admin.filters != 0 || admin.far > 0.0 || !admin.hidden.is_empty();
    if !active && !admin.had_filters { return; }
    admin.had_filters = active;
    let eye = cameras.iter().next().map(|g| g.translation());
    for (e, at, reference, land, sky, cloud, actor, water, grass, tree, leaves, bark, class, layers, hidden) in &meshes {
        let category = if land { LAND } else if sky { SKY | if cloud { CLOUDS } else { 0 } }
            else if actor || class.is_some_and(|c| *c == crate::draw_distance::DrawClass::Actor) { ACTORS } else if water { WATER } else if grass { GRASS }
            else if tree || leaves || bark { TREES | MODELS } else { MODELS };
        let hide = admin.filters & category != 0
            || reference.is_some_and(|r| admin.hidden.contains(&r.0))
            || (admin.far > 0.0 && category & MODELS != 0 && eye.is_some_and(|eye| at.translation().distance(eye) > admin.far));
        if hide && hidden.is_none() {
            commands.entity(e).insert((AdminHidden(layers.cloned()), RenderLayers::layer(30)));
        } else if !hide {
            if let Some(AdminHidden(old)) = hidden {
                let mut entity = commands.entity(e);
                entity.remove::<AdminHidden>();
                if let Some(old) = old { entity.insert(old.clone()); } else { entity.remove::<RenderLayers>(); }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ray_boxes_include_rotated_model_space_and_parallel_misses() {
        assert_eq!(ray_box(Vec3::new(0.0, 0.0, 10.0), Vec3::NEG_Z, Vec3::splat(-1.0), Vec3::ONE), Some(9.0));
        assert_eq!(ray_box(Vec3::new(4.0, 0.0, 10.0), Vec3::NEG_Z, Vec3::splat(-1.0), Vec3::ONE), None);
        assert_eq!(ray_box(Vec3::ZERO, Vec3::X, Vec3::splat(-1.0), Vec3::ONE), Some(0.0));
    }
    #[test]
    fn individual_hide_is_reversible_and_undo_does_not_repeat_ids() {
        let mut admin = Admin::default();
        hide_reference(&mut admin, 42); hide_reference(&mut admin, 99); hide_reference(&mut admin, 42);
        assert_eq!(admin.hidden, BTreeSet::from([99]));
        assert_eq!(admin.undo, vec![99]);
    }
}
