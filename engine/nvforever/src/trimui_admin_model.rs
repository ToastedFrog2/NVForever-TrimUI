//! Pure workbench operations, testable without the renderer or device.
use std::collections::BTreeSet;

/// User configuration overrides the shipped default. Missing/invalid values stay off.
pub fn dev_menu_enabled(packaged: Option<&str>, user: Option<&str>) -> bool {
    let Some(text) = user.or(packaged) else { return false };
    let (mut section, mut enabled) = (false, false);
    for line in text.trim_start_matches('\u{feff}').lines() {
        let line = line.split([';', '#']).next().unwrap_or("").trim();
        if line.starts_with('[') && line.ends_with(']') {
            section = line[1..line.len()-1].trim().eq_ignore_ascii_case("DeveloperMenu");
        } else if section {
            if let Some((key, value)) = line.split_once('=') {
                if key.trim().eq_ignore_ascii_case("Enabled") {
                    enabled = matches!(value.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on");
                }
            }
        }
    }
    enabled
}
pub fn ray_box(origin: [f32;3], direction: [f32;3], min: [f32;3], max: [f32;3]) -> Option<f32> {
    let (mut near, mut far) = (0.0f32, 20000.0f32);
    for i in 0..3 {
        if direction[i].abs() < 1e-8 {
            if origin[i] < min[i] || origin[i] > max[i] { return None; }
        } else {
            let (a,b) = ((min[i]-origin[i])/direction[i], (max[i]-origin[i])/direction[i]);
            near = near.max(a.min(b)); far = far.min(a.max(b));
            if near > far { return None; }
        }
    }
    Some(near)
}
pub fn toggle_hidden(hidden: &mut BTreeSet<u32>, undo: &mut Vec<u32>, id: u32) {
    if id == 0x14 { return; }
    if hidden.insert(id) { undo.push(id); }
    else { hidden.remove(&id); undo.retain(|r| *r != id); }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn developer_menu_requires_explicit_enable_in_its_section() {
        for text in ["", "Enabled=1", "[Other]\nEnabled=1", "[DeveloperMenu]\nEnabled=0", "[DeveloperMenu]\nEnabled=garbage"] {
            assert!(!dev_menu_enabled(Some(text),None));
        }
        assert!(dev_menu_enabled(Some("\u{feff}[developermenu]\n enabled = YES ; explicit opt-in"),None));
    }
    #[test]
    fn developer_menu_user_override_survives_packaged_defaults() {
        let on="[DeveloperMenu]\nEnabled=1";
        let off="[DeveloperMenu]\nEnabled=0";
        assert!(dev_menu_enabled(Some(off),Some(on)));
        assert!(!dev_menu_enabled(Some(on),Some(off)));
        assert!(!dev_menu_enabled(Some(on),Some("broken user config")));
        assert!(!dev_menu_enabled(None,None));
    }
    #[test]
    fn box_pick_handles_parallel_inside_and_backwards_rays() {
        assert_eq!(ray_box([0.,0.,10.],[0.,0.,-1.],[-1.;3],[1.;3]),Some(9.));
        assert_eq!(ray_box([4.,0.,10.],[0.,0.,-1.],[-1.;3],[1.;3]),None);
        assert_eq!(ray_box([0.;3],[1.,0.,0.],[-1.;3],[1.;3]),Some(0.));
        assert_eq!(ray_box([0.,0.,10.],[0.,0.,1.],[-1.;3],[1.;3]),None);
    }
    #[test]
    fn scaled_ray_keeps_world_distance() {
        assert_eq!(ray_box([0.,0.,5.],[0.,0.,-0.5],[-1.;3],[1.;3]),Some(8.));
    }
    #[test]
    fn hide_restore_rehide_maintains_undo_and_protects_player() {
        let (mut hidden,mut undo)=(BTreeSet::new(),Vec::new());
        for id in [42,99,42,42,0x14] { toggle_hidden(&mut hidden,&mut undo,id); }
        assert_eq!(undo,vec![99,42]); assert_eq!(hidden,BTreeSet::from([42,99]));
    }
}
