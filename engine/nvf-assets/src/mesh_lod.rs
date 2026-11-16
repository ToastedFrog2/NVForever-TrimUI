//! Simpler versions of a model's big meshes, for the lean renderer on the TrimUI
//! (`NVF_MESH_LOD`; docs/trimui.md "November 10").
//!
//! On the handheld's PowerVR GE8300 the GPU's time follows the triangles drawn, about
//! 70 ns each [observed 2026-11-10: at the owner's save outside Goodsprings the frame drew
//! about 389,000 triangles, 323,000 of them in ~150 placed-model meshes of 500 triangles and
//! up; hiding those took the GPU from 98 % to 65 % busy]. A big mesh far away covers few
//! pixels with many triangles. This builds, when the model is prepared, one simpler copy of
//! each big mesh (meshoptimizer's edge-collapse simplifier, borders locked so pieces placed
//! side by side keep meeting) with how far its surface strays from the original's, in game
//! units; nvforever (`trimui.rs` `switch_mesh_lods`) draws the copy where that stray covers
//! less than a set fraction of a pixel.
//!
//! [engineering choice, not the original's behaviour: the original draws placed models at
//! full detail up to their fade distance and has no per-mesh levels of detail.]

use bevy::mesh::{Indices, PrimitiveTopology, VertexAttributeValues};
use bevy::prelude::*;

/// What [`simplified`] makes, from `NVF_MESH_LOD=<min triangles>,<share kept>,<max error>`
/// (lean renderer only; unset or malformed: no simpler meshes).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LodSettings {
    /// Meshes with fewer triangles are left as they are.
    pub min_triangles: usize,
    /// The share of the triangles the simplifier aims for (0–1).
    pub keep: f32,
    /// The largest error the simplifier may reach, as a share of the mesh's size.
    pub max_error: f32,
}

/// [`LodSettings`] from `NVF_MESH_LOD`, read once; `None` without the lean renderer.
pub fn settings() -> Option<LodSettings> {
    static S: std::sync::OnceLock<Option<LodSettings>> = std::sync::OnceLock::new();
    *S.get_or_init(|| {
        if !crate::nif_material::basic_rendering() {
            return None;
        }
        let s = parse(&std::env::var("NVF_MESH_LOD").ok()?)?;
        info!("mesh LOD: meshes of {}+ triangles get a copy with about {:.0} % of them (error up to {:.1} % of the mesh's size)", s.min_triangles, s.keep * 100.0, s.max_error * 100.0);
        Some(s)
    })
}

fn parse(v: &str) -> Option<LodSettings> {
    let mut it = v.split(',').map(str::trim);
    let min_triangles = it.next()?.parse().ok()?;
    let keep: f32 = it.next()?.parse().ok()?;
    let max_error: f32 = it.next()?.parse().ok()?;
    (keep > 0.0 && keep < 1.0 && max_error > 0.0 && max_error.is_finite()).then_some(LodSettings { min_triangles, keep, max_error })
}

/// A simpler copy of `mesh` and how far its surface may stray from the original's, in the
/// mesh's own units; `None` when the mesh is not a plain indexed triangle list of float
/// attributes, or the simplifier cannot take away at least a quarter of its triangles.
/// The copy keeps only the vertices its triangles use.
pub fn simplified(mesh: &Mesh, s: &LodSettings) -> Option<(Mesh, f32)> {
    if mesh.primitive_topology() != PrimitiveTopology::TriangleList {
        return None;
    }
    let Some(VertexAttributeValues::Float32x3(positions)) = mesh.attribute(Mesh::ATTRIBUTE_POSITION) else { return None };
    let indices: Vec<u32> = match mesh.indices()? {
        Indices::U16(i) => i.iter().map(|&v| v as u32).collect(),
        Indices::U32(i) => i.clone(),
    };
    if indices.len() / 3 < s.min_triangles {
        return None;
    }
    // Every attribute must be one this can carry over vertex by vertex.
    if !mesh.attributes().all(|(_, v)| matches!(v, VertexAttributeValues::Float32x2(_) | VertexAttributeValues::Float32x3(_) | VertexAttributeValues::Float32x4(_))) {
        return None;
    }
    let bytes: &[u8] = bytemuck_bytes(positions);
    let adapter = meshopt::VertexDataAdapter::new(bytes, 12, 0).ok()?;
    let target = ((indices.len() as f32 * s.keep) as usize / 3) * 3;
    let mut error = 0.0f32;
    let lod = meshopt::simplify(&indices, &adapter, target, s.max_error, meshopt::SimplifyOptions::LockBorder, Some(&mut error));
    if lod.is_empty() || lod.len() * 4 > indices.len() * 3 {
        return None;
    }
    let scale = meshopt::simplify_scale(&adapter);
    // The vertices the copy uses, in the order its triangles first use them.
    let mut remap = vec![u32::MAX; positions.len()];
    let mut order: Vec<u32> = Vec::new();
    let new_indices: Vec<u32> = lod.iter().map(|&i| {
        let slot = &mut remap[i as usize];
        if *slot == u32::MAX {
            *slot = order.len() as u32;
            order.push(i);
        }
        *slot
    }).collect();
    let mut out = Mesh::new(PrimitiveTopology::TriangleList, mesh.asset_usage);
    for (attr, values) in mesh.attributes() {
        let picked = match values {
            VertexAttributeValues::Float32x2(v) => VertexAttributeValues::Float32x2(order.iter().map(|&i| v[i as usize]).collect()),
            VertexAttributeValues::Float32x3(v) => VertexAttributeValues::Float32x3(order.iter().map(|&i| v[i as usize]).collect()),
            VertexAttributeValues::Float32x4(v) => VertexAttributeValues::Float32x4(order.iter().map(|&i| v[i as usize]).collect()),
            _ => return None,
        };
        out.insert_attribute(attr.clone(), picked);
    }
    out.insert_indices(crate::nif_spawn::compact_indices(new_indices, order.len()));
    Some((out, error * scale))
}

fn bytemuck_bytes(v: &[[f32; 3]]) -> &[u8] {
    // SAFETY: [f32; 3] is plain old data with no padding; the slice covers exactly its bytes.
    unsafe { std::slice::from_raw_parts(v.as_ptr().cast::<u8>(), std::mem::size_of_val(v)) }
}

/// A placed shape's two meshes: the full one (`near`) and its simpler copy (`far`), drawn
/// from the distance at which `error` (game units) covers less than the chosen part of a
/// pixel (nvforever `trimui.rs` `switch_mesh_lods`).
#[derive(Component, Clone, Debug)]
pub struct MeshLod {
    pub near: Handle<Mesh>,
    pub far: Handle<Mesh>,
    pub error: f32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::asset::RenderAssetUsages;

    fn plain_mesh() -> Mesh {
        Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD)
    }

    /// A bumpy 41 × 41 grid: 3200 triangles.
    fn grid() -> Mesh {
        let n = 41usize;
        let mut pos = Vec::new();
        let mut uv = Vec::new();
        for y in 0..n {
            for x in 0..n {
                let h = ((x as f32 * 0.3).sin() + (y as f32 * 0.2).cos()) * 0.05;
                pos.push([x as f32, y as f32, h]);
                uv.push([x as f32 / 40.0, y as f32 / 40.0]);
            }
        }
        let mut idx = Vec::new();
        for y in 0..n - 1 {
            for x in 0..n - 1 {
                let a = (y * n + x) as u32;
                let b = a + n as u32;
                idx.extend_from_slice(&[a, a + 1, b, a + 1, b + 1, b]);
            }
        }
        plain_mesh()
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, pos)
            .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uv)
            .with_inserted_indices(Indices::U32(idx))
    }

    #[test]
    fn settings_parse() {
        assert_eq!(parse("1500, 0.25, 0.02"), Some(LodSettings { min_triangles: 1500, keep: 0.25, max_error: 0.02 }));
        assert_eq!(parse("1500,1.5,0.02"), None);
        assert_eq!(parse("1500,0.25"), None);
        assert_eq!(parse("x"), None);
    }

    #[test]
    fn big_meshes_get_a_smaller_copy_with_its_attributes() {
        let s = LodSettings { min_triangles: 1000, keep: 0.25, max_error: 0.05 };
        let (lod, error) = simplified(&grid(), &s).expect("a copy");
        let tris = lod.indices().unwrap().len() / 3;
        assert!(tris <= 3200 * 3 / 4, "{tris} triangles");
        assert!(error >= 0.0 && error < 0.05 * 60.0);
        let verts = lod.count_vertices();
        assert!(verts < 41 * 41);
        assert!(matches!(lod.attribute(Mesh::ATTRIBUTE_UV_0), Some(VertexAttributeValues::Float32x2(v)) if v.len() == verts));
        assert!(lod.indices().unwrap().iter().all(|i| i < verts));
        // The locked border: the grid's four corners are still there.
        let Some(VertexAttributeValues::Float32x3(p)) = lod.attribute(Mesh::ATTRIBUTE_POSITION) else { panic!() };
        for c in [[0.0, 0.0], [40.0, 0.0], [0.0, 40.0], [40.0, 40.0]] {
            assert!(p.iter().any(|v| v[0] == c[0] && v[1] == c[1]), "corner {c:?}");
        }
    }

    #[test]
    fn small_meshes_and_other_formats_are_left_alone() {
        let s = LodSettings { min_triangles: 5000, keep: 0.25, max_error: 0.05 };
        assert!(simplified(&grid(), &s).is_none());
        let s = LodSettings { min_triangles: 100, ..s };
        let colored = grid().with_inserted_attribute(Mesh::ATTRIBUTE_JOINT_INDEX, bevy::mesh::VertexAttributeValues::Uint16x4(vec![[0; 4]; 41 * 41]));
        assert!(simplified(&colored, &s).is_none());
    }
}
