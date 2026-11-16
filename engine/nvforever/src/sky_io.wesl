// What the basic renderer's sky vertex stage (sky_vertex.wesl) hands sky.wesl's fragment
// stage under NVF_BASIC (sky.rs `specialize`): the clip position, texture coordinates and
// vertex colour at Bevy's forward locations, and `sun`, the view ray's coordinates on the
// sun's plane (x, y across it, z along the sun's direction), from which the fragment finds
// the sun and glare textures' coordinates with one divide.
struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @if(VERTEX_UVS_A)
    @location(2) uv: vec2<f32>,
    @if(VERTEX_COLORS)
    @location(5) color: vec4<f32>,
    @location(8) sun: vec3<f32>,
}
