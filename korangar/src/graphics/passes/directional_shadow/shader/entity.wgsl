struct PassUniforms {
    view_projection: mat4x4<f32>,
    animation_timer: f32,
}

struct InstanceData {
    world: mat4x4<f32>,
    texture_top_left: vec2<f32>,
    texture_bottom_left: vec2<f32>,
    texture_top_right: vec2<f32>,
    texture_bottom_right: vec2<f32>,
    texture_position: vec2<f32>,
    texture_size: vec2<f32>,
    depth_offset: f32,
    depth_extra: f32,
    angle: f32,
    foo: f32,
    foo_2: f32,
    curvature: f32,
    mirror: u32,
    texture_index: i32,
}

struct Vertex {
    position: vec3<f32>,
    texture_coordinates: vec2<f32>,
    depth_multiplier: f32,
    curvature_multiplier: f32,
}

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) texture_coordinates: vec2<f32>,
    @location(1) depth_offset: f32,
    @location(2) curvature: f32,
    @location(3) @interpolate(flat) original_depth_offset: f32,
    @location(4) @interpolate(flat) original_curvature: f32,
    @location(5) angle: f32,
}

@group(0) @binding(1) var nearest_sampler: sampler;
@group(1) @binding(0) var<uniform> pass_uniforms: PassUniforms;
@group(2) @binding(0) var<storage, read> instance_data: array<InstanceData>;
@group(3) @binding(0) var texture: texture_2d<f32>;

override near_plane: f32;

// Small value to prevent division by zero.
const epsilon: f32 = 1e-7;

@vertex
fn vs_main(
    @builtin(vertex_index) vertex_index: u32,
    @builtin(instance_index) instance_index: u32,
) -> VertexOutput {
    let instance = instance_data[instance_index];
    let vertex = vertex_data_new(vertex_index, instance_index);

    var output: VertexOutput;
    output.position = pass_uniforms.view_projection * instance.world * vec4<f32>(vertex.position, 1.0);
    output.texture_coordinates = instance.texture_position + vertex.texture_coordinates * instance.texture_size;

    if (instance.mirror != 0u) {
        output.texture_coordinates.x = 1.0 - output.texture_coordinates.x;
    }

    output.depth_offset = vertex.depth_multiplier;
    output.curvature = vertex.curvature_multiplier;
    output.original_depth_offset = instance.depth_offset;
    output.original_curvature = instance.curvature;
    output.angle = instance.angle;
    return output;
}

@fragment
fn fs_main(input: VertexOutput) -> @builtin(frag_depth) f32 {
    var new_input = input.texture_coordinates;
    if abs(input.angle) > 0.0001 {
        let sin_factor = sin(input.angle);
        let cos_factor = cos(input.angle);
        let rotate = vec2(input.texture_coordinates.x - 0.5, input.texture_coordinates.y - 0.5) * mat2x2(cos_factor, sin_factor, -sin_factor, cos_factor);
        new_input = vec2(clamp(rotate.x + 0.5, 0.0, 1.0), clamp(rotate.y + 0.5, 0.0, 1.0));
    } 

    let diffuse_color = textureSample(texture, nearest_sampler, new_input);
    if (diffuse_color.a != 1.0) {
        discard;
    }

    let scaled_depth_offset = pow(input.depth_offset, 2.0) * input.original_depth_offset;
    let scaled_curvature_offset = (0.5 - pow(input.curvature, 2.0)) * input.original_curvature;

    let linear_z: f32 = nonLinearToLinear(input.position.z);
    // We add the offsets in linear view space.
    let adjusted_linear_z: f32 = 2.0 + linear_z - scaled_curvature_offset - scaled_curvature_offset;
    let non_linear_z: f32 = linearToNonLinear(adjusted_linear_z);
    let clamped_depth = clamp(non_linear_z, 0.0, 1.0);

    return input.position.z;
    // FIX: we don't even use any of the calculated values here! should it not be:
    // return clamped_depth;
}

// Optimized version of the following truth table:
//
// vertex_index  x  y  z  u  v  d  c
// 0            -1  2  1  0  0  1 -1
// 1            -1  0  1  0  1  0 -1
// 2             1  2  1  1  0  1  1
// 3             1  2  1  1  0  1  1
// 4            -1  0  1  0  1  0 -1
// 5             1  0  1  1  1  0  1
//
// (x,y,z) are the vertex position
// (u,v) are the UV coordinates
// (depth) is the depth multiplier
// (curve) is the cuvature multiplier
fn vertex_data(vertex_index: u32) -> Vertex {
    let index = 1u << vertex_index;

    let case0 = i32((index & 0x13u) != 0u);
    let case1 = i32((index & 0x0Du) != 0u);

    let x = f32(1 - 2 * case0);
    let y = f32(2 * case1);
    let z = 1.0;
    let u = f32(1 - case0);
    let v = f32(1 - case1);
    let depth = f32(case1);
    let curve = u * 2.0 - 1.0;

    return Vertex(vec3<f32>(x, y, z), vec2<f32>(u, v), depth, curve);
}

fn vertex_data_new(vertex_index: u32, instance_index: u32) -> Vertex {
    let instance = instance_data[instance_index];
    let depth_extra = instance.depth_extra;
    if (vertex_index == 0) {
        let x = instance.texture_top_left.x;
        let y = instance.texture_top_left.y;
        let z = 1.0;
        let u = 0.0;
        let v = 0.0;
        let depth = y / 2.0 + depth_extra;
        let curve = x;
        return Vertex(vec3<f32>(x, y, z), vec2<f32>(u, v), depth, curve);
    }
    else if (vertex_index == 1 || vertex_index == 4) {
        let x = instance.texture_bottom_left.x;
        let y = instance.texture_bottom_left.y;
        let z = 1.0;
        let u = 0.0;
        let v = 1.0;
        let depth = y / 2.0 + depth_extra;
        let curve = x;
        return Vertex(vec3<f32>(x, y, z), vec2<f32>(u, v), depth, curve);
    }
    else if (vertex_index == 2 || vertex_index == 3) {
        let x = instance.texture_top_right.x;
        let y = instance.texture_top_right.y;
        let z = 1.0;
        let u = 1.0;
        let v = 0.0;
        let depth = y / 2.0 + depth_extra;
        let curve = x;
        return Vertex(vec3<f32>(x, y, z), vec2<f32>(u, v), depth, curve);
    } else {
        let x = instance.texture_bottom_right.x;
        let y = instance.texture_bottom_right.y;
        let z = 1.0;
        let u = 1.0;
        let v = 1.0;
        let depth = y / 2.0 + depth_extra;
        let curve = x;
        return Vertex(vec3<f32>(x, y, z), vec2<f32>(u, v), depth, curve);
    }
}

fn linearToNonLinear(linear_depth: f32) -> f32 {
    return near_plane / (linear_depth + epsilon);
}

fn nonLinearToLinear(non_linear_depth: f32) -> f32 {
    return near_plane / (non_linear_depth + epsilon);
}
