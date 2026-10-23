// The terrain: block faces from the CPU mesher (`mesher.rs`), textured from
// one 2D texture array, tinted, alpha-tested or blended, and shaded by face.
//
// Colour maths happens in linear light. The texture array is sampled through
// an sRGB view, so texels arrive linear; tints arrive sRGB-encoded and are
// converted here; the sRGB surface encodes the result on the way out.

struct Frame {
    // Rotation and projection only: positions arrive camera-relative.
    view_projection: mat4x4<f32>,
}

@group(0) @binding(0) var<uniform> frame: Frame;
@group(0) @binding(1) var block_textures: texture_2d_array<f32>;
@group(0) @binding(2) var block_sampler: sampler;

struct Vertex {
    // x, y and z in sixteenths of a block within the sub-chunk, then the
    // texture layer.
    @location(0) position_layer: vec4<u32>,
    // u and v in sixteenths of the texture, the face (bits 0 to 2) and
    // class (bits 3 and 4), and a spare byte.
    @location(1) uv_face: vec4<u32>,
    // The tint, sRGB-encoded, with the blend opacity in alpha.
    @location(2) tint: vec4<f32>,
    // Per draw: the sub-chunk's origin minus the camera, in blocks.
    @location(3) offset: vec4<f32>,
}

struct Varyings {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) @interpolate(flat) layer: u32,
    @location(2) @interpolate(flat) render_class: u32,
    @location(3) @interpolate(flat) shade: f32,
    @location(4) tint: vec4<f32>,
}

// Render classes, as `mesher::Vertex` packs them.
const CUTOUT: u32 = 1u;
const OVERLAY: u32 = 2u;
const BLEND: u32 = 3u;

// Constant brightness by face, as Minecraft looks without smooth lighting:
// sRGB factors of 0.5 below, 1 above, 0.8 north and south, 0.6 west and east,
// and 1 for a plant's diagonal planes, raised to 2.2 for linear light. It
// isn't lighting (that's M5); it keeps cubes readable.
fn face_shade(face: u32) -> f32 {
    switch face {
        case 0u: { return 0.218; }
        case 2u, 3u: { return 0.612; }
        case 4u, 5u: { return 0.325; }
        default: { return 1.0; }
    }
}

// The sRGB transfer function, inverted.
fn srgb_to_linear(c: vec3<f32>) -> vec3<f32> {
    return select(pow((c + 0.055) / 1.055, vec3<f32>(2.4)), c / 12.92, c <= vec3<f32>(0.04045));
}

@vertex
fn vertex_main(v: Vertex) -> Varyings {
    var out: Varyings;
    let local = vec3<f32>(v.position_layer.xyz) / 16.0;
    out.clip = frame.view_projection * vec4<f32>(local + v.offset.xyz, 1.0);
    out.uv = vec2<f32>(v.uv_face.xy) / 16.0;
    out.layer = v.position_layer.w;
    out.render_class = v.uv_face.z >> 3u;
    out.shade = face_shade(v.uv_face.z & 7u);
    out.tint = vec4<f32>(srgb_to_linear(v.tint.rgb), v.tint.a);
    return out;
}

@fragment
fn fragment_main(in: Varyings) -> @location(0) vec4<f32> {
    // Sampled first, while every invocation still runs: mipmapping needs
    // the neighbouring pixels' coordinates.
    let texel = textureSample(block_textures, block_sampler, in.uv, in.layer);
    var color = texel.rgb * in.tint.rgb;
    var alpha = 1.0;
    switch in.render_class {
        case CUTOUT: {
            if texel.a < 0.5 {
                discard;
            }
        }
        case OVERLAY: {
            // Drawn opaque; alpha marks the texels that take the tint.
            color = texel.rgb * mix(vec3<f32>(1.0), in.tint.rgb, texel.a);
        }
        case BLEND: {
            alpha = texel.a * in.tint.a;
        }
        default: {}
    }
    return vec4<f32>(color * in.shade, alpha);
}
