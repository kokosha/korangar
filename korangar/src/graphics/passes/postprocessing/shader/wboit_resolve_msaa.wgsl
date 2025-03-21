@group(1) @binding(0) var accumulation_texture: texture_multisampled_2d<f32>;
@group(2) @binding(0) var revealage_texture: texture_multisampled_2d<f32>;

override MSAA_SAMPLE_COUNT: i32;

@vertex
fn vs_main(@builtin(vertex_index) vertex_index: u32) -> @builtin(position) vec4<f32> {
    // Full screen triangle.
    let uv = vec2<f32>(f32((vertex_index << 1u) & 2u), f32(vertex_index & 2u));
    return vec4<f32>(uv * vec2<f32>(2.0, -2.0) + vec2<f32>(-1.0, 1.0), 0.0, 1.0);
}

@fragment
fn fs_main(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let pixel_coord = vec2<i32>(position.xy);
    var color: vec4<f32>;

    for (var index: i32 = 0; index < MSAA_SAMPLE_COUNT; index++) {
        let accumulation = textureLoad(accumulation_texture, pixel_coord, index);
        let revealage = textureLoad(revealage_texture, pixel_coord, index).r;
        color += vec4<f32>(accumulation.rgb / max(accumulation.a, 1e-5), revealage);
    }

    color = color / f32(MSAA_SAMPLE_COUNT);

    if (color.a > 0.99) {
        discard;
    }

    return color;
}
