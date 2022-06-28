// Vertex shader

struct Resolution {
    x: f32;
    y: f32;
};
[[group(0), binding(0)]] // 2.
var<uniform> Nyasolution: Resolution;


struct VertexInput {
    [[location(0)]] position: vec3<f32>;
};

struct VertexOutput {
    [[builtin(position)]] clip_position: vec4<f32>;
};

[[stage(vertex)]]
fn vs_main(
    model: VertexInput,
) -> VertexOutput {
    var out: VertexOutput;
    out.clip_position = vec4<f32>(model.position, 1.0);
    return out;
}

[[stage(fragment)]]
fn fs_main(in: VertexOutput) -> [[location(0)]] vec4<f32> {

    let resolution : vec2<f32> = vec2<f32>(Nyasolution.x, Nyasolution.y);
	let uv : vec2<f32> = in.clip_position.xy / resolution.xy;
    let uv : vec2<f32> =  uv * (1.0 - uv.yx);
    let vig : f32 = uv.x*uv.y * 10.0;
    let vig : f32 = pow(vig, 0.25);

    return vec4<f32>(0., 0., 0., 1.1-vig);
}
