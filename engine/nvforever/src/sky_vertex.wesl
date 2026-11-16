// The sky's vertex stage under the basic renderer (sky.rs `specialize`, the TrimUI build):
// Bevy's mesh vertex stage with the outputs sky.wesl reads there (sky_io.wesl).
//
// The sun disc and its glare are textures spread over squares around the sun
// (sky.wesl `sun_texel`): the fragment's coordinates are (dot(d, u), -dot(d, v)) /
// dot(d, s) for the view ray d, the sun's direction s and two directions u, v across it
// (the ray's point on the plane through s, perpendicular to it; d needs no normalizing,
// the ratio is the same). The three dot products are linear in the world position, so
// interpolated across a triangle they are exact at every fragment, and the fragment is
// left one divide where it normalized, crossed and projected for each sky part and layer
// [engineering choice: the same picture; the cloud layers were about 13 ms of the frame
// at 1280x720 on the PowerVR GE8300, 2026-11-11].
import bevy_pbr::{
    render::mesh_functions,
    render::forward_io::{Vertex, decompress_vertex},
    render::view_transformations::position_world_to_clip,
    render::mesh_view_bindings::view,
};
import nvforever::sky_io::VertexOutput;

// sky.wesl's SkyLight (sky.rs `SkyLight`); only `sun_dir` is read here.
struct SkyLight {
    params: vec4<f32>,
    glow: vec4<f32>,
    sun_dir: vec4<f32>,
    sun: vec4<f32>,
    glare: vec4<f32>,
}

@group(constants::MATERIAL_BIND_GROUP) @binding(100) var<uniform> sky: SkyLight;

@vertex
fn vertex(vertex_in: Vertex) -> VertexOutput {
    var out: VertexOutput;
    let vertex = decompress_vertex(vertex_in, vertex_in.instance_index);
    let world_from_local = mesh_functions::get_world_from_local(vertex_in.instance_index);
    let world_position = mesh_functions::mesh_position_local_to_world(world_from_local, vec4<f32>(vertex.position, 1.0));
    out.position = position_world_to_clip(world_position.xyz);
@if(VERTEX_UVS_A)
    out.uv = vertex.uv;
@if(VERTEX_COLORS)
    out.color = vertex.color;
    // The same frame across the sun as sky.wesl `sun_texel` builds.
    let sd = normalize(sky.sun_dir.xyz);
    var up = vec3(0.0, 1.0, 0.0);
    if abs(sd.y) > 0.999 {
        up = vec3(0.0, 0.0, 1.0);
    }
    let u = normalize(cross(up, sd));
    let v = cross(sd, u);
    let d = world_position.xyz - view.world_position.xyz;
    out.sun = vec3(dot(d, u), -dot(d, v), dot(d, sd));
    return out;
}
