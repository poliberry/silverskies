//! GPU renderer for the 3D RADAR pane (OpenGL, inside egui).
//!
//! * **Terrain**: the level-of-detail leaves as depth-tested meshes, shaded
//!   by elevation (a dark hypsometric tint, hillshading and faint contour
//!   lines), fading into the sky with distance.
//! * **Radar**: ray-marched as a volume through every pixel. A NEXRAD Level
//!   II scan is sampled where it was measured, in the radar's own polar
//!   coordinates (range, azimuth and tilt, with 4/3-earth beam bending),
//!   blending between neighbouring tilts; elsewhere the mosaic's 3D grid is
//!   sampled. Rays stop at the ground. Colour and opacity come from the
//!   reflectivity scale.
//!
//! GL objects live behind a mutex shared with the paint callback, which
//! uploads what's new and draws.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use eframe::egui_glow;
use eframe::glow::{self, HasContext};
use egui::Rect;
use ss_core::radar3d::{PolarVolume, Scene, AZIMUTHS};

use super::radar3d::{dbz_color, V3};
use super::terrain_lod::{Leaf, LeafKey, LEAF_N};

/// Vertical field of view (matches `View`).
const FOV_DEG: f32 = 45.0;
/// Top of the radar volume, km above sea level.
const VOLUME_TOP_KM: f32 = 20.0;
/// Most tilts the shader takes.
const MAX_TILTS: usize = 24;

/// Everything one frame draws.
pub struct Frame {
    pub eye: V3,
    pub right: V3,
    pub up: V3,
    pub fwd: V3,
    pub ex: f32,
    pub dark: bool,
    pub min_dbz: f32,
    /// What colours the echoes (opacity always follows reflectivity).
    pub product: Product,
    /// Storm motion (east, north, m/s), for storm-relative velocity.
    pub motion: [f32; 2],
    pub terrain: bool,
    pub radar: bool,
    /// Drape satellite imagery (each leaf carries its own).
    pub photo: bool,
    pub leaves: Vec<Leaf>,
    /// The scene and an id for its radar data (changes with each scan).
    pub scene: Arc<Scene>,
    pub scene_id: String,
    /// Sky colour at the horizon, for haze.
    pub haze: [f32; 3],
}

pub use ss_core::radar3d::L2Product as Product;

/// The shader's code for a product.
fn product_code(p: Product) -> i32 {
    match p {
        Product::Reflectivity => 0,
        Product::Velocity => 1,
        Product::StormRelative => 2,
        Product::Zdr => 3,
        Product::Cc => 4,
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Status {
    /// Not tried yet.
    Unknown,
    Ready,
    /// Shaders didn't build here (e.g. OpenGL 2): draw on the CPU instead.
    Failed(String),
}

struct GpuLeaf {
    vbo: glow::Buffer,
    ibo: glow::Buffer,
    count: i32,
    used: u64,
}

struct Programs {
    terrain: glow::Program,
    volume: glow::Program,
    /// Copies the reduced-size volume image onto the pane.
    blit: glow::Program,
    vao: glow::VertexArray,
}

/// The ray-march runs on at most this many pixels; a bigger pane (an
/// undocked or maximised window) gets it scaled up, so its cost stays
/// that of a docked pane.
const MAX_VOLUME_PIXELS: f32 = 360_000.0;

const BLIT_FS: &str = r#"
uniform vec4 u_viewport;
uniform sampler2D u_tex;
out vec4 frag;
void main() {
    vec2 uv = (gl_FragCoord.xy - u_viewport.xy) / u_viewport.zw;
    frag = texture(u_tex, uv);
}
"#;

/// The textures for one scan.
struct Volume {
    id: String,
    /// The product in its second channel.
    product: Product,
    /// Polar tilts (a 2D texture array) or the mosaic grid (3D texture).
    tex: glow::Texture,
    polar: Option<PolarMeta>,
    grid: Option<GridMeta>,
}

struct PolarMeta {
    radar: [f32; 3],
    width: usize,
    elev: Vec<f32>,
    first: Vec<f32>,
    gate: Vec<f32>,
}

struct GridMeta {
    half: f32,
    top: f32,
}

#[derive(Default)]
struct Inner {
    status: Option<Status>,
    progs: Option<Programs>,
    leaves: HashMap<LeafKey, GpuLeaf>,
    volume: Option<Volume>,
    ground: Option<(String, glow::Texture, usize, f32)>,
    lut: Option<(i32, glow::Texture)>,
    /// The product colour scale, by product.
    plut: Option<(i32, glow::Texture)>,
    /// Off-screen target for the reduced-size ray-march: (fbo, colour
    /// texture, width, height).
    target: Option<(glow::Framebuffer, glow::Texture, i32, i32)>,
    frame: u64,
}

/// Shared between the pane and its paint callbacks.
#[derive(Clone, Default)]
pub struct Gl3d {
    inner: Arc<Mutex<Inner>>,
}

impl Gl3d {
    pub fn status(&self) -> Status {
        self.inner
            .lock()
            .ok()
            .and_then(|i| i.status.clone())
            .unwrap_or(Status::Unknown)
    }

    /// The paint callback drawing `frame` into `rect`.
    pub fn callback(&self, rect: Rect, frame: Frame) -> egui::PaintCallback {
        let inner = self.inner.clone();
        let frame = Arc::new(frame);
        egui::PaintCallback {
            rect,
            callback: Arc::new(egui_glow::CallbackFn::new(move |info, painter| {
                let Ok(mut i) = inner.lock() else { return };
                // SAFETY: called by egui_glow on its GL thread with its
                // context current; all objects used were made in it.
                unsafe { i.paint(painter, &info, &frame) };
            })),
        }
    }
}

// --- Matrices -------------------------------------------------------------------

/// Column-major view-projection for the camera.
fn view_proj(f: &Frame, aspect: f32) -> [f32; 16] {
    let (r, u, b) = (f.right, f.up, V3::new(-f.fwd.x, -f.fwd.y, -f.fwd.z));
    let e = f.eye;
    let view = [
        r.x,
        u.x,
        b.x,
        0.0, //
        r.y,
        u.y,
        b.y,
        0.0, //
        r.z,
        u.z,
        b.z,
        0.0, //
        -r.dot(e),
        -u.dot(e),
        -b.dot(e),
        1.0,
    ];
    let (near, far) = (0.01f32, 4000.0f32);
    let t = 1.0 / (FOV_DEG.to_radians() / 2.0).tan();
    let proj = [
        t / aspect,
        0.0,
        0.0,
        0.0, //
        0.0,
        t,
        0.0,
        0.0, //
        0.0,
        0.0,
        (far + near) / (near - far),
        -1.0, //
        0.0,
        0.0,
        2.0 * far * near / (near - far),
        0.0,
    ];
    let mut m = [0.0f32; 16];
    for c in 0..4 {
        for row in 0..4 {
            m[c * 4 + row] = (0..4).map(|k| proj[k * 4 + row] * view[c * 4 + k]).sum();
        }
    }
    m
}

// --- Shaders --------------------------------------------------------------------

const TERRAIN_VS: &str = r#"
in vec3 a_pos;
in vec3 a_extra;
in vec2 a_uv;
uniform mat4 u_vp;
uniform float u_ex;
out vec2 v_uv;
out vec3 v_world;
out vec3 v_normal;
out float v_h;
out float v_skirt;
void main() {
    v_skirt = a_extra.z > 0.0 ? 1.0 : 0.0;
    v_uv = a_uv;
    vec3 p = vec3(a_pos.xy, (a_pos.z - a_extra.z) * u_ex);
    v_world = p;
    v_h = a_pos.z;
    v_normal = normalize(vec3(-a_extra.x * u_ex, -a_extra.y * u_ex, 1.0));
    gl_Position = u_vp * vec4(p, 1.0);
}
"#;

const TERRAIN_FS: &str = r#"
in vec3 v_world;
in vec3 v_normal;
in float v_h;
in float v_skirt;
in vec2 v_uv;
uniform vec3 u_eye;
uniform vec3 u_haze;
uniform float u_dark;
uniform int u_photo;
uniform vec4 u_uvrect;
uniform sampler2D u_img;

out vec4 frag;

vec3 tint(float h) {
    // Elevation (km): sea, lowland, hills, mountains, high peaks.
    vec3 dk[5] = vec3[5](vec3(0.047, 0.078, 0.133), vec3(0.106, 0.137, 0.180),
        vec3(0.157, 0.188, 0.239), vec3(0.239, 0.267, 0.322), vec3(0.47, 0.50, 0.56));
    vec3 lt[5] = vec3[5](vec3(0.66, 0.75, 0.84), vec3(0.80, 0.83, 0.76),
        vec3(0.74, 0.72, 0.62), vec3(0.62, 0.58, 0.52), vec3(0.93, 0.93, 0.94));
    float hs[5] = float[5](0.0, 0.05, 0.6, 1.8, 3.6);
    vec3 c0 = mix(lt[0], dk[0], u_dark);
    if (h <= 0.002) return c0;
    vec3 c = mix(lt[1], dk[1], u_dark);
    for (int i = 1; i < 4; i++) {
        vec3 a = mix(lt[i], dk[i], u_dark);
        vec3 b = mix(lt[i + 1], dk[i + 1], u_dark);
        if (h >= hs[i]) c = mix(a, b, clamp((h - hs[i]) / (hs[i + 1] - hs[i]), 0.0, 1.0));
    }
    return c;
}

void main() {
    // The surface's own slope (continuous across leaves), leaning on the
    // vertex normal so close-ups don't look faceted.
    vec3 fn_ = normalize(cross(dFdx(v_world), dFdy(v_world)));
    if (fn_.z < 0.0) fn_ = -fn_;
    vec3 n = normalize(fn_ * 0.75 + normalize(v_normal) * 0.25);
    // Skirts (walls hiding cracks between leaves) look like the ground above.
    if (v_skirt > 0.0) n = normalize(v_normal);
    vec3 l = normalize(vec3(-0.5, 0.6, 0.65));
    float shade = 0.55 + 0.6 * max(dot(n, l), 0.0) - 0.15 * (1.0 - n.z);
    vec3 c = tint(v_h) * shade;
    if (u_photo == 1) {
        // The photo has its own light: only a gentle slope shading.
        // egui textures hold display (sRGB) values as they are.
        vec3 img = texture(u_img, u_uvrect.xy + v_uv * u_uvrect.zw).rgb;
        c = img * clamp(0.75 + 0.35 * dot(n, l) / l.z - 0.25, 0.6, 1.08);
    } else if (v_h > 0.002) {
        // Faint contours every 100 m (fading where they'd crowd).
        float f = v_h * 10.0;
        float w = fwidth(f);
        float line = 1.0 - smoothstep(0.0, 1.5 * w, abs(fract(f + 0.5) - 0.5));
        c = mix(c, c * (u_dark > 0.5 ? 1.35 : 0.82), line * clamp(1.0 - w * 4.0, 0.0, 1.0) * 0.6);
    }
    float d = length(v_world - u_eye);
    float fog = 1.0 - exp(-d / 420.0);
    frag = vec4(mix(c, u_haze, fog * 0.85), 1.0);
}
"#;

const VOLUME_VS: &str = r#"
void main() {
    // One triangle covering the viewport.
    vec2 p = vec2(float((gl_VertexID << 1) & 2), float(gl_VertexID & 2));
    gl_Position = vec4(p * 2.0 - 1.0, 0.0, 1.0);
}
"#;

const VOLUME_FS: &str = r#"
uniform vec4 u_viewport;
uniform vec3 u_eye;
uniform vec3 u_right;
uniform vec3 u_up;
uniform vec3 u_fwd;
uniform float u_tan;
uniform float u_aspect;
uniform float u_ex;
uniform float u_half;
uniform float u_top;
uniform float u_min;
uniform int u_mode;          // 0 polar, 1 grid
uniform int u_product;       // 0 reflectivity, 1 velocity, 2 storm-relative, 3 ZDR, 4 CC
uniform vec2 u_prange;       // the product's encoded range
uniform vec2 u_motion;       // storm motion, m/s east and north
uniform sampler2D u_plut;
uniform sampler2DArray u_polar;
uniform sampler3D u_grid;
uniform sampler2D u_lut;
uniform sampler2D u_ground;
uniform float u_ground_n;
uniform float u_ground_cell;
uniform int u_tilts;
uniform float u_elev[24];
uniform float u_first[24];
uniform float u_gate[24];
uniform float u_width;
uniform vec3 u_radar;
uniform float u_grid_half;
uniform float u_grid_top;
out vec4 frag;

float ground(vec2 xy) {
    float n = u_ground_n;
    if (n < 2.0) return 0.0;
    float half_t = (n - 1.0) * u_ground_cell * 0.5;
    vec2 f = vec2((xy.x + half_t) / u_ground_cell, (half_t - xy.y) / u_ground_cell);
    f = clamp(f, vec2(0.0), vec2(n - 1.001));
    ivec2 i = ivec2(floor(f));
    vec2 t = f - vec2(i);
    float a = texelFetch(u_ground, i, 0).r;
    float b = texelFetch(u_ground, i + ivec2(1, 0), 0).r;
    float c = texelFetch(u_ground, i + ivec2(0, 1), 0).r;
    float d = texelFetch(u_ground, i + ivec2(1, 1), 0).r;
    return mix(mix(a, b, t.x), mix(c, d, t.x), t.y) / 1000.0;
}

// Encoded reflectivity (0 = none, else (dBZ + 32) * 2) and product at a
// tilt.
vec2 tilt(int k, float r, float v) {
    float u = ((r - u_first[k]) / u_gate[k] + 0.5) / u_width;
    if (u < 0.0 || u > 1.0) return vec2(0.0);
    return texture(u_polar, vec3(u, v, float(k))).rg * 255.0;
}

// Blend two samples; a product value only blends with another one.
vec2 blend(vec2 a, vec2 b, float t) {
    float r = mix(a.x, b.x, t);
    float p = a.y < 0.5 ? b.y : (b.y < 0.5 ? a.y : mix(a.y, b.y, t));
    return vec2(r, p);
}

vec2 polar(vec3 p) {
    vec3 d = p - u_radar;
    float s = length(d.xy);
    float h = d.z;
    // Beam elevation through this point, with standard refraction.
    float e = degrees(atan(h - s * s / (2.0 * 8494.7), s));
    float r = sqrt(s * s + h * h);
    float az = atan(d.x, d.y);
    if (az < 0.0) az += 6.2831853;
    float v = az / 6.2831853;
    int n = u_tilts;
    if (n == 0 || e < u_elev[0] - 0.5 || e > u_elev[n - 1] + 0.5) return vec2(0.0);
    int k = 0;
    for (int i = 1; i < 24; i++) {
        if (i >= n) break;
        if (e >= u_elev[i]) k = i;
    }
    vec2 a = tilt(k, r, v);
    if (k + 1 < n && e > u_elev[k]) {
        float gap = u_elev[k + 1] - u_elev[k];
        float t = (e - u_elev[k]) / gap;
        if (gap <= 3.0) {
            // Between close tilts: blend across.
            return blend(a, tilt(k + 1, r, v), t);
        }
        // Far apart: each tilt fills its own beam (~1°) only.
        if (e - u_elev[k] < 0.6) return a;
        if (u_elev[k + 1] - e < 0.6) return tilt(k + 1, r, v);
        return vec2(0.0);
    }
    return a;
}

float grid(vec3 p) {
    vec3 t = vec3((p.x + u_grid_half) / (2.0 * u_grid_half),
                  (p.y + u_grid_half) / (2.0 * u_grid_half), p.z / u_grid_top);
    if (any(lessThan(t, vec3(0.0))) || any(greaterThan(t, vec3(1.0)))) return 0.0;
    return texture(u_grid, t).r * 255.0;
}

float hash(vec2 p) {
    return fract(sin(dot(p, vec2(12.9898, 78.233))) * 43758.5453);
}

void main() {
    vec2 ndc = (gl_FragCoord.xy - u_viewport.xy) / u_viewport.zw * 2.0 - 1.0;
    vec3 dir = normalize(u_fwd + u_right * ndc.x * u_tan * u_aspect + u_up * ndc.y * u_tan);
    // The volume's box (display space: heights times the exaggeration).
    vec3 lo = vec3(-u_half, -u_half, -0.5);
    vec3 hi = vec3(u_half, u_half, u_top * u_ex);
    vec3 inv = 1.0 / dir;
    vec3 t0 = (lo - u_eye) * inv, t1 = (hi - u_eye) * inv;
    vec3 tn = min(t0, t1), tf = max(t0, t1);
    float tnear = max(max(tn.x, tn.y), max(tn.z, 0.0));
    float tfar = min(min(tf.x, tf.y), tf.z);
    if (tnear >= tfar) discard;
    float enc_min = (u_min + 32.0) * 2.0;
    vec4 acc = vec4(0.0);
    float t = tnear;
    float jitter = hash(gl_FragCoord.xy);
    for (int i = 0; i < 900; i++) {
        float step = clamp(t * 0.004, 0.08, 1.2);
        float ts = t + step * jitter;
        if (ts > tfar) break;
        vec3 p = u_eye + dir * ts;
        vec3 q = vec3(p.xy, p.z / u_ex);
        if (q.z < ground(q.xy)) break;
        vec2 e = u_mode == 0 ? polar(q) : vec2(grid(q), 0.0);
        if (e.x >= enc_min) {
            vec4 c = texture(u_lut, vec2(e.x / 255.0, 0.5));
            if (u_product > 0) {
                // Colour from the product; opacity still from reflectivity.
                if (e.y < 0.5) { t += step; continue; }
                float val = u_prange.x + (e.y - 1.0) / 254.0 * (u_prange.y - u_prange.x);
                if (u_product == 2) {
                    vec2 rd = q.xy - u_radar.xy;
                    float len = length(rd);
                    if (len > 0.001) val -= dot(u_motion, rd / len);
                }
                float f = clamp((val - u_prange.x) / (u_prange.y - u_prange.x), 0.0, 1.0);
                c.rgb = texture(u_plut, vec2(f, 0.5)).rgb;
            }
            float a = 1.0 - exp(-c.a * step);
            acc.rgb += (1.0 - acc.a) * a * c.rgb;
            acc.a += (1.0 - acc.a) * a;
            if (acc.a > 0.97) break;
        }
        t += step;
    }
    if (acc.a < 0.003) discard;
    frag = acc;
}
"#;

fn header(gl: &glow::Context) -> Option<String> {
    let v = egui_glow::ShaderVersion::get(gl);
    if !v.is_new_shader_interface() {
        return None;
    }
    let mut h = v.version_declaration().to_string();
    if h.contains(" es") {
        h.push_str(
            "precision highp float;\nprecision highp int;\nprecision highp sampler2DArray;\n\
             precision highp sampler3D;\n",
        );
    }
    Some(h)
}

unsafe fn program(
    gl: &glow::Context,
    head: &str,
    vs: &str,
    fs: &str,
) -> Result<glow::Program, String> {
    unsafe {
        let prog = gl.create_program()?;
        let mut shaders = Vec::new();
        for (kind, src) in [(glow::VERTEX_SHADER, vs), (glow::FRAGMENT_SHADER, fs)] {
            let s = gl.create_shader(kind)?;
            gl.shader_source(s, &format!("{head}{src}"));
            gl.compile_shader(s);
            if !gl.get_shader_compile_status(s) {
                return Err(gl.get_shader_info_log(s));
            }
            gl.attach_shader(prog, s);
            shaders.push(s);
        }
        gl.link_program(prog);
        if !gl.get_program_link_status(prog) {
            return Err(gl.get_program_info_log(prog));
        }
        for s in shaders {
            gl.detach_shader(prog, s);
            gl.delete_shader(s);
        }
        Ok(prog)
    }
}

/// The reflectivity colour scale as a 256-entry texture indexed by the
/// encoded value; alpha is extinction per km.
fn lut_pixels() -> Vec<u8> {
    let mut px = Vec::with_capacity(256 * 4);
    for b in 0..256u32 {
        let dbz = b as f32 / 2.0 - 32.0;
        let c = dbz_color(dbz.max(5.0));
        // Light rain a thin veil, cores nearly solid within a km or two.
        let k = ((dbz - 5.0) / 55.0).clamp(0.0, 1.0);
        let density = 0.04 + 2.2 * k * k;
        px.extend_from_slice(&[
            c.r(),
            c.g(),
            c.b(),
            (density / 2.5 * 255.0).min(255.0) as u8,
        ]);
    }
    px
}

impl Inner {
    unsafe fn setup(&mut self, gl: &glow::Context) -> Result<(), String> {
        let head = header(gl).ok_or("OpenGL 3 is needed for the 3D renderer")?;
        unsafe {
            let terrain = program(gl, &head, TERRAIN_VS, TERRAIN_FS)?;
            let volume = program(gl, &head, VOLUME_VS, VOLUME_FS)?;
            let blit = program(gl, &head, VOLUME_VS, BLIT_FS)?;
            let vao = gl.create_vertex_array()?;
            self.progs = Some(Programs {
                terrain,
                volume,
                blit,
                vao,
            });
        }
        Ok(())
    }

    unsafe fn paint(
        &mut self,
        painter: &egui_glow::Painter,
        info: &egui::PaintCallbackInfo,
        f: &Frame,
    ) {
        let gl: &glow::Context = painter.gl();
        self.frame += 1;
        if self.status.is_none() {
            let s = match unsafe { self.setup(gl) } {
                Ok(()) => Status::Ready,
                Err(e) => {
                    tracing::warn!("3D renderer: {e}");
                    Status::Failed(e)
                }
            };
            self.status = Some(s);
        }
        if self.status != Some(Status::Ready) {
            return;
        }
        let vp = info.viewport_in_pixels();
        let clip = info.clip_rect_in_pixels();
        // `SS_GL3D_TIMING=1`: log how long each frame takes (testing).
        let timing = std::env::var_os("SS_GL3D_TIMING").is_some();
        let t0 = std::time::Instant::now();
        let aspect = vp.width_px as f32 / vp.height_px.max(1) as f32;
        unsafe {
            gl.enable(glow::SCISSOR_TEST);
            gl.scissor(
                clip.left_px,
                clip.from_bottom_px,
                clip.width_px,
                clip.height_px,
            );
            gl.depth_mask(true);
            gl.clear_depth_f32(1.0);
            gl.clear(glow::DEPTH_BUFFER_BIT);
            if f.terrain {
                self.draw_terrain(painter, f, aspect);
            }
            if f.radar {
                let full = [vp.left_px, vp.from_bottom_px, vp.width_px, vp.height_px];
                let px = (vp.width_px * vp.height_px) as f32;
                // `SS_GL3D_MAX_PX` overrides the cap (testing).
                let cap = std::env::var("SS_GL3D_MAX_PX")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(MAX_VOLUME_PIXELS);
                let scale = (cap / px.max(1.0)).sqrt();
                if scale >= 0.95 {
                    self.draw_volume(gl, f, full, aspect);
                } else {
                    let w = ((vp.width_px as f32 * scale) as i32).max(1);
                    let h = ((vp.height_px as f32 * scale) as i32).max(1);
                    self.draw_volume_scaled(gl, f, full, [w, h], aspect, &clip);
                }
            }
            gl.depth_mask(true);
            gl.disable(glow::DEPTH_TEST);
            gl.bind_vertex_array(None);
            gl.bind_buffer(glow::ARRAY_BUFFER, None);
            gl.bind_buffer(glow::ELEMENT_ARRAY_BUFFER, None);
            gl.active_texture(glow::TEXTURE0);
            if timing {
                gl.finish();
                tracing::info!(
                    "3D frame {}x{}: {:.1} ms",
                    vp.width_px,
                    vp.height_px,
                    t0.elapsed().as_secs_f64() * 1000.0
                );
            }
        }
        self.evict(gl);
    }

    unsafe fn draw_terrain(&mut self, painter: &egui_glow::Painter, f: &Frame, aspect: f32) {
        let gl: &glow::Context = painter.gl();
        let Some(p) = &self.progs else { return };
        let (prog, vao) = (p.terrain, p.vao);
        // Upload new leaves (a few per frame, so a fresh view fills in
        // without a stall).
        let mut budget = 64;
        for leaf in &f.leaves {
            if self.leaves.contains_key(&leaf.key) {
                continue;
            }
            if budget == 0 {
                break;
            }
            budget -= 1;
            let (verts, idx) = leaf_mesh(&leaf.pts);
            unsafe {
                let (Ok(vbo), Ok(ibo)) = (gl.create_buffer(), gl.create_buffer()) else {
                    return;
                };
                gl.bind_buffer(glow::ARRAY_BUFFER, Some(vbo));
                gl.buffer_data_u8_slice(glow::ARRAY_BUFFER, bytes_f32(&verts), glow::STATIC_DRAW);
                gl.bind_buffer(glow::ELEMENT_ARRAY_BUFFER, Some(ibo));
                gl.buffer_data_u8_slice(
                    glow::ELEMENT_ARRAY_BUFFER,
                    bytes_u32(&idx),
                    glow::STATIC_DRAW,
                );
                self.leaves.insert(
                    leaf.key,
                    GpuLeaf {
                        vbo,
                        ibo,
                        count: idx.len() as i32,
                        used: self.frame,
                    },
                );
            }
        }
        unsafe {
            gl.use_program(Some(prog));
            gl.bind_vertex_array(Some(vao));
            gl.enable(glow::DEPTH_TEST);
            gl.depth_func(glow::LEQUAL);
            gl.disable(glow::BLEND);
            gl.disable(glow::CULL_FACE);
            let m = view_proj(f, aspect);
            gl.uniform_matrix_4_f32_slice(
                gl.get_uniform_location(prog, "u_vp").as_ref(),
                false,
                &m,
            );
            gl.uniform_1_f32(gl.get_uniform_location(prog, "u_ex").as_ref(), f.ex);
            gl.uniform_3_f32(
                gl.get_uniform_location(prog, "u_eye").as_ref(),
                f.eye.x,
                f.eye.y,
                f.eye.z,
            );
            gl.uniform_3_f32(
                gl.get_uniform_location(prog, "u_haze").as_ref(),
                f.haze[0],
                f.haze[1],
                f.haze[2],
            );
            gl.uniform_1_f32(
                gl.get_uniform_location(prog, "u_dark").as_ref(),
                if f.dark { 1.0 } else { 0.0 },
            );
            let a_pos = gl.get_attrib_location(prog, "a_pos").unwrap_or(0);
            let a_extra = gl.get_attrib_location(prog, "a_extra").unwrap_or(1);
            let a_uv = gl.get_attrib_location(prog, "a_uv").unwrap_or(2);
            let u_photo = gl.get_uniform_location(prog, "u_photo");
            let u_uvrect = gl.get_uniform_location(prog, "u_uvrect");
            gl.uniform_1_i32(gl.get_uniform_location(prog, "u_img").as_ref(), 0);
            gl.active_texture(glow::TEXTURE0);
            for leaf in &f.leaves {
                let Some(g) = self.leaves.get_mut(&leaf.key) else {
                    continue;
                };
                g.used = self.frame;
                let img = leaf
                    .img
                    .filter(|_| f.photo)
                    .and_then(|(id, r)| painter.texture(id).map(|t| (t, r)));
                match img {
                    Some((tex, r)) => {
                        gl.bind_texture(glow::TEXTURE_2D, Some(tex));
                        gl.uniform_1_i32(u_photo.as_ref(), 1);
                        gl.uniform_4_f32(
                            u_uvrect.as_ref(),
                            r.min.x,
                            r.min.y,
                            r.width(),
                            r.height(),
                        );
                    }
                    None => gl.uniform_1_i32(u_photo.as_ref(), 0),
                }
                gl.bind_buffer(glow::ARRAY_BUFFER, Some(g.vbo));
                gl.enable_vertex_attrib_array(a_pos);
                gl.vertex_attrib_pointer_f32(a_pos, 3, glow::FLOAT, false, 32, 0);
                gl.enable_vertex_attrib_array(a_extra);
                gl.vertex_attrib_pointer_f32(a_extra, 3, glow::FLOAT, false, 32, 12);
                gl.enable_vertex_attrib_array(a_uv);
                gl.vertex_attrib_pointer_f32(a_uv, 2, glow::FLOAT, false, 32, 24);
                gl.bind_buffer(glow::ELEMENT_ARRAY_BUFFER, Some(g.ibo));
                gl.draw_elements(glow::TRIANGLES, g.count, glow::UNSIGNED_INT, 0);
            }
            gl.disable_vertex_attrib_array(a_pos);
            gl.disable_vertex_attrib_array(a_extra);
            gl.disable_vertex_attrib_array(a_uv);
            gl.bind_texture(glow::TEXTURE_2D, None);
        }
    }

    /// Upload the scan (once per scan), the ground and the colour scale.
    unsafe fn prepare_volume(&mut self, gl: &glow::Context, f: &Frame) -> bool {
        unsafe {
            gl.pixel_store_i32(glow::UNPACK_ALIGNMENT, 1);
            let vid = format!("{}:{:?}", f.scene_id, f.product.moment());
            if self.volume.as_ref().is_none_or(|v| v.id != vid) {
                if let Some(old) = self.volume.take() {
                    gl.delete_texture(old.tex);
                }
                self.volume = upload_volume(gl, &f.scene, &vid, f.product);
            }
            let gid = format!(
                "{}:{}",
                f.scene_id,
                f.scene.terrain.as_ref().map_or(0, |t| t.n)
            );
            if self.ground.as_ref().is_none_or(|g| g.0 != gid) {
                if let Some(old) = self.ground.take() {
                    gl.delete_texture(old.1);
                }
                if let (Some(t), Ok(tex)) = (&f.scene.terrain, gl.create_texture()) {
                    gl.bind_texture(glow::TEXTURE_2D, Some(tex));
                    set_filter(gl, glow::TEXTURE_2D, glow::NEAREST);
                    gl.tex_image_2d(
                        glow::TEXTURE_2D,
                        0,
                        glow::R32F as i32,
                        t.n as i32,
                        t.n as i32,
                        0,
                        glow::RED,
                        glow::FLOAT,
                        glow::PixelUnpackData::Slice(Some(bytes_f32(&t.heights_m))),
                    );
                    self.ground = Some((gid, tex, t.n, t.cell_km));
                }
            }
            let code = product_code(f.product);
            if self.plut.as_ref().is_none_or(|(c, _)| *c != code) {
                if let Some((_, old)) = self.plut.take() {
                    gl.delete_texture(old);
                }
                if let Ok(tex) = gl.create_texture() {
                    let (lo, hi) = f.product.moment().range();
                    let px: Vec<u8> = (0..256)
                        .flat_map(|i| {
                            let v = lo + (hi - lo) * i as f32 / 255.0;
                            let c = f.product.color(v);
                            [c[0], c[1], c[2], 255]
                        })
                        .collect();
                    gl.bind_texture(glow::TEXTURE_2D, Some(tex));
                    set_filter(gl, glow::TEXTURE_2D, glow::LINEAR);
                    gl.tex_image_2d(
                        glow::TEXTURE_2D,
                        0,
                        glow::RGBA8 as i32,
                        256,
                        1,
                        0,
                        glow::RGBA,
                        glow::UNSIGNED_BYTE,
                        glow::PixelUnpackData::Slice(Some(&px)),
                    );
                    self.plut = Some((code, tex));
                }
            }
            if self.lut.is_none() {
                if let Ok(tex) = gl.create_texture() {
                    gl.bind_texture(glow::TEXTURE_2D, Some(tex));
                    set_filter(gl, glow::TEXTURE_2D, glow::LINEAR);
                    gl.tex_image_2d(
                        glow::TEXTURE_2D,
                        0,
                        glow::RGBA8 as i32,
                        256,
                        1,
                        0,
                        glow::RGBA,
                        glow::UNSIGNED_BYTE,
                        glow::PixelUnpackData::Slice(Some(&lut_pixels())),
                    );
                    self.lut = Some((0, tex));
                }
            }
            gl.pixel_store_i32(glow::UNPACK_ALIGNMENT, 4);
        }
        self.volume.is_some() && self.lut.is_some() && self.plut.is_some()
    }

    /// Ray-march into a `size` image off screen, then scale it onto `vp`.
    unsafe fn draw_volume_scaled(
        &mut self,
        gl: &glow::Context,
        f: &Frame,
        vp: [i32; 4],
        size: [i32; 2],
        aspect: f32,
        clip: &egui::epaint::ViewportInPixels,
    ) {
        unsafe {
            // (Re)make the target at this size.
            if self.target.is_none_or(|t| (t.2, t.3) != (size[0], size[1])) {
                if let Some((fb, tex, _, _)) = self.target.take() {
                    gl.delete_framebuffer(fb);
                    gl.delete_texture(tex);
                }
                let (Ok(fb), Ok(tex)) = (gl.create_framebuffer(), gl.create_texture()) else {
                    return;
                };
                gl.bind_texture(glow::TEXTURE_2D, Some(tex));
                gl.tex_image_2d(
                    glow::TEXTURE_2D,
                    0,
                    glow::RGBA8 as i32,
                    size[0],
                    size[1],
                    0,
                    glow::RGBA,
                    glow::UNSIGNED_BYTE,
                    glow::PixelUnpackData::Slice(None),
                );
                for (k, v) in [
                    (glow::TEXTURE_MIN_FILTER, glow::LINEAR),
                    (glow::TEXTURE_MAG_FILTER, glow::LINEAR),
                    (glow::TEXTURE_WRAP_S, glow::CLAMP_TO_EDGE),
                    (glow::TEXTURE_WRAP_T, glow::CLAMP_TO_EDGE),
                ] {
                    gl.tex_parameter_i32(glow::TEXTURE_2D, k, v as i32);
                }
                gl.bind_texture(glow::TEXTURE_2D, None);
                let prev = gl.get_parameter_framebuffer(glow::FRAMEBUFFER_BINDING);
                gl.bind_framebuffer(glow::FRAMEBUFFER, Some(fb));
                gl.framebuffer_texture_2d(
                    glow::FRAMEBUFFER,
                    glow::COLOR_ATTACHMENT0,
                    glow::TEXTURE_2D,
                    Some(tex),
                    0,
                );
                let ok =
                    gl.check_framebuffer_status(glow::FRAMEBUFFER) == glow::FRAMEBUFFER_COMPLETE;
                gl.bind_framebuffer(glow::FRAMEBUFFER, prev);
                if !ok {
                    gl.delete_framebuffer(fb);
                    gl.delete_texture(tex);
                    // Can't render off screen here: full size it is.
                    self.draw_volume(gl, f, vp, aspect);
                    return;
                }
                self.target = Some((fb, tex, size[0], size[1]));
            }
            let Some((fb, tex, _, _)) = self.target else {
                return;
            };
            // March into the small image.
            let prev = gl.get_parameter_framebuffer(glow::FRAMEBUFFER_BINDING);
            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(fb));
            gl.disable(glow::SCISSOR_TEST);
            gl.viewport(0, 0, size[0], size[1]);
            gl.clear_color(0.0, 0.0, 0.0, 0.0);
            gl.clear(glow::COLOR_BUFFER_BIT);
            self.draw_volume(gl, f, [0, 0, size[0], size[1]], aspect);
            // And scale it onto the pane.
            gl.bind_framebuffer(glow::FRAMEBUFFER, prev);
            gl.viewport(vp[0], vp[1], vp[2], vp[3]);
            gl.enable(glow::SCISSOR_TEST);
            gl.scissor(
                clip.left_px,
                clip.from_bottom_px,
                clip.width_px,
                clip.height_px,
            );
            let Some(p) = &self.progs else { return };
            gl.use_program(Some(p.blit));
            gl.bind_vertex_array(Some(p.vao));
            gl.disable(glow::DEPTH_TEST);
            gl.enable(glow::BLEND);
            gl.blend_func(glow::ONE, glow::ONE_MINUS_SRC_ALPHA);
            let u = |n: &str| gl.get_uniform_location(p.blit, n);
            gl.uniform_4_f32(
                u("u_viewport").as_ref(),
                vp[0] as f32,
                vp[1] as f32,
                vp[2] as f32,
                vp[3] as f32,
            );
            gl.uniform_1_i32(u("u_tex").as_ref(), 0);
            gl.active_texture(glow::TEXTURE0);
            gl.bind_texture(glow::TEXTURE_2D, Some(tex));
            gl.draw_arrays(glow::TRIANGLES, 0, 3);
            gl.bind_texture(glow::TEXTURE_2D, None);
        }
    }

    unsafe fn draw_volume(&mut self, gl: &glow::Context, f: &Frame, vp: [i32; 4], aspect: f32) {
        if !unsafe { self.prepare_volume(gl, f) } {
            return;
        }
        let (Some(p), Some(vol), Some((_, lut)), Some((_, plut))) =
            (&self.progs, &self.volume, &self.lut, &self.plut)
        else {
            return;
        };
        let prog = p.volume;
        unsafe {
            gl.use_program(Some(prog));
            gl.bind_vertex_array(Some(p.vao));
            gl.disable(glow::DEPTH_TEST);
            gl.enable(glow::BLEND);
            gl.blend_func(glow::ONE, glow::ONE_MINUS_SRC_ALPHA);
            let u = |n: &str| gl.get_uniform_location(prog, n);
            gl.uniform_4_f32(
                u("u_viewport").as_ref(),
                vp[0] as f32,
                vp[1] as f32,
                vp[2] as f32,
                vp[3] as f32,
            );
            for (name, v) in [
                ("u_eye", f.eye),
                ("u_right", f.right),
                ("u_up", f.up),
                ("u_fwd", f.fwd),
            ] {
                gl.uniform_3_f32(u(name).as_ref(), v.x, v.y, v.z);
            }
            gl.uniform_1_f32(u("u_tan").as_ref(), (FOV_DEG.to_radians() / 2.0).tan());
            gl.uniform_1_f32(u("u_aspect").as_ref(), aspect);
            gl.uniform_1_f32(u("u_ex").as_ref(), f.ex);
            gl.uniform_1_f32(u("u_half").as_ref(), f.scene.half_km);
            gl.uniform_1_f32(u("u_top").as_ref(), VOLUME_TOP_KM);
            gl.uniform_1_f32(u("u_min").as_ref(), f.min_dbz);
            // Texture units: 0 polar, 1 grid, 2 colour scale, 3 ground.
            gl.uniform_1_i32(u("u_polar").as_ref(), 0);
            gl.uniform_1_i32(u("u_grid").as_ref(), 1);
            gl.uniform_1_i32(u("u_lut").as_ref(), 2);
            gl.uniform_1_i32(u("u_ground").as_ref(), 3);
            gl.uniform_1_i32(u("u_plut").as_ref(), 4);
            let product = if vol.polar.is_some() {
                vol.product
            } else {
                Product::Reflectivity
            };
            gl.uniform_1_i32(u("u_product").as_ref(), product_code(product));
            let (lo, hi) = product.moment().range();
            gl.uniform_2_f32(u("u_prange").as_ref(), lo, hi);
            gl.uniform_2_f32(u("u_motion").as_ref(), f.motion[0], f.motion[1]);
            gl.active_texture(glow::TEXTURE4);
            gl.bind_texture(glow::TEXTURE_2D, Some(*plut));
            if let Some(pm) = &vol.polar {
                gl.uniform_1_i32(u("u_mode").as_ref(), 0);
                gl.active_texture(glow::TEXTURE0);
                gl.bind_texture(glow::TEXTURE_2D_ARRAY, Some(vol.tex));
                gl.uniform_1_i32(u("u_tilts").as_ref(), pm.elev.len() as i32);
                let pad = |v: &[f32]| {
                    let mut a = [0.0f32; MAX_TILTS];
                    for (o, x) in a.iter_mut().zip(v) {
                        *o = *x;
                    }
                    a
                };
                gl.uniform_1_f32_slice(u("u_elev").as_ref(), &pad(&pm.elev));
                gl.uniform_1_f32_slice(u("u_first").as_ref(), &pad(&pm.first));
                gl.uniform_1_f32_slice(u("u_gate").as_ref(), &pad(&pm.gate));
                gl.uniform_1_f32(u("u_width").as_ref(), pm.width as f32);
                gl.uniform_3_f32(u("u_radar").as_ref(), pm.radar[0], pm.radar[1], pm.radar[2]);
            } else if let Some(gm) = &vol.grid {
                gl.uniform_1_i32(u("u_mode").as_ref(), 1);
                gl.active_texture(glow::TEXTURE1);
                gl.bind_texture(glow::TEXTURE_3D, Some(vol.tex));
                gl.uniform_1_f32(u("u_grid_half").as_ref(), gm.half);
                gl.uniform_1_f32(u("u_grid_top").as_ref(), gm.top);
            }
            gl.active_texture(glow::TEXTURE2);
            gl.bind_texture(glow::TEXTURE_2D, Some(*lut));
            gl.active_texture(glow::TEXTURE3);
            match &self.ground {
                Some((_, tex, n, cell)) => {
                    gl.bind_texture(glow::TEXTURE_2D, Some(*tex));
                    gl.uniform_1_f32(u("u_ground_n").as_ref(), *n as f32);
                    gl.uniform_1_f32(u("u_ground_cell").as_ref(), *cell);
                }
                None => gl.uniform_1_f32(u("u_ground_n").as_ref(), 0.0),
            }
            gl.draw_arrays(glow::TRIANGLES, 0, 3);
            for unit in [
                glow::TEXTURE4,
                glow::TEXTURE3,
                glow::TEXTURE2,
                glow::TEXTURE1,
                glow::TEXTURE0,
            ] {
                gl.active_texture(unit);
                gl.bind_texture(glow::TEXTURE_2D, None);
            }
            gl.bind_texture(glow::TEXTURE_2D_ARRAY, None);
            gl.active_texture(glow::TEXTURE1);
            gl.bind_texture(glow::TEXTURE_3D, None);
            gl.active_texture(glow::TEXTURE0);
        }
    }

    /// Drop leaves unused for a while once there are many.
    fn evict(&mut self, gl: &glow::Context) {
        if self.leaves.len() < 1500 {
            return;
        }
        let now = self.frame;
        let old: Vec<LeafKey> = self
            .leaves
            .iter()
            .filter(|(_, g)| g.used + 120 < now)
            .map(|(k, _)| *k)
            .collect();
        for k in old {
            if let Some(g) = self.leaves.remove(&k) {
                // SAFETY: as in `paint`.
                unsafe {
                    gl.delete_buffer(g.vbo);
                    gl.delete_buffer(g.ibo);
                }
            }
        }
    }
}

unsafe fn set_filter(gl: &glow::Context, target: u32, filter: u32) {
    unsafe {
        gl.tex_parameter_i32(target, glow::TEXTURE_MIN_FILTER, filter as i32);
        gl.tex_parameter_i32(target, glow::TEXTURE_MAG_FILTER, filter as i32);
        gl.tex_parameter_i32(target, glow::TEXTURE_WRAP_S, glow::CLAMP_TO_EDGE as i32);
        gl.tex_parameter_i32(target, glow::TEXTURE_WRAP_T, glow::CLAMP_TO_EDGE as i32);
        gl.tex_parameter_i32(target, glow::TEXTURE_WRAP_R, glow::CLAMP_TO_EDGE as i32);
    }
}

unsafe fn upload_volume(
    gl: &glow::Context,
    scene: &Scene,
    id: &str,
    product: Product,
) -> Option<Volume> {
    unsafe {
        let tex = gl.create_texture().ok()?;
        if let Some(p) = scene.polar.as_ref().filter(|p| !p.sweeps.is_empty()) {
            let PolarVolume {
                radar,
                width,
                sweeps,
                data,
                ..
            } = p;
            // Reflectivity and the product, side by side; a product is
            // sampled gate by gate (blending velocities would invent values).
            let second = p.moment(product.moment()).unwrap_or(data);
            let n = sweeps.len().min(MAX_TILTS);
            let len = width * AZIMUTHS * n;
            let mut rg = Vec::with_capacity(len * 2);
            for (a, b) in data[..len].iter().zip(&second[..len]) {
                rg.push(*a);
                rg.push(*b);
            }
            gl.bind_texture(glow::TEXTURE_2D_ARRAY, Some(tex));
            let filter = if product == Product::Reflectivity {
                glow::LINEAR
            } else {
                glow::NEAREST
            };
            set_filter(gl, glow::TEXTURE_2D_ARRAY, filter);
            // Azimuth wraps around north.
            gl.tex_parameter_i32(
                glow::TEXTURE_2D_ARRAY,
                glow::TEXTURE_WRAP_T,
                glow::REPEAT as i32,
            );
            gl.tex_image_3d(
                glow::TEXTURE_2D_ARRAY,
                0,
                glow::RG8 as i32,
                *width as i32,
                AZIMUTHS as i32,
                n as i32,
                0,
                glow::RG,
                glow::UNSIGNED_BYTE,
                glow::PixelUnpackData::Slice(Some(&rg)),
            );
            let s = &sweeps[..n];
            return Some(Volume {
                id: id.to_string(),
                product,
                tex,
                polar: Some(PolarMeta {
                    radar: *radar,
                    width: *width,
                    elev: s.iter().map(|w| w.elevation_deg).collect(),
                    first: s.iter().map(|w| w.first_gate_km).collect(),
                    gate: s.iter().map(|w| w.gate_km).collect(),
                }),
                grid: None,
            });
        }
        // The mosaic (or a scan without polar data): its voxels on a grid.
        let km = scene.voxel_km.max(0.25);
        let dz = scene.voxel_dz_km.max(0.1);
        let half = scene.half_km;
        let nxy = ((2.0 * half / km).ceil() as usize).clamp(1, 1024);
        let nz = ((VOLUME_TOP_KM / dz).ceil() as usize).clamp(1, 256);
        let mut data = vec![0u8; nxy * nxy * nz];
        for v in &scene.voxels {
            let i = ((v.x + half) / km) as isize;
            let j = ((v.y + half) / km) as isize;
            let k = (v.z / dz) as isize;
            if i < 0 || j < 0 || k < 0 || i >= nxy as isize || j >= nxy as isize || k >= nz as isize
            {
                continue;
            }
            let at = (k as usize * nxy + j as usize) * nxy + i as usize;
            let b = ((v.dbz + 32.0) * 2.0).round().clamp(1.0, 255.0) as u8;
            data[at] = data[at].max(b);
        }
        gl.bind_texture(glow::TEXTURE_3D, Some(tex));
        set_filter(gl, glow::TEXTURE_3D, glow::LINEAR);
        gl.tex_image_3d(
            glow::TEXTURE_3D,
            0,
            glow::R8 as i32,
            nxy as i32,
            nxy as i32,
            nz as i32,
            0,
            glow::RED,
            glow::UNSIGNED_BYTE,
            glow::PixelUnpackData::Slice(Some(&data)),
        );
        Some(Volume {
            id: id.to_string(),
            product: Product::Reflectivity,
            tex,
            polar: None,
            grid: Some(GridMeta {
                half,
                top: nz as f32 * dz,
            }),
        })
    }
}

/// A leaf's vertices (position; height gradient and skirt drop) and
/// triangles, skirts included.
fn leaf_mesh(pts: &[[f32; 3]]) -> (Vec<f32>, Vec<u32>) {
    let n = LEAF_N;
    let at = |i: usize, j: usize| pts[j.min(n - 1) * n + i.min(n - 1)];
    let mut v = Vec::with_capacity((n * n + 4 * n) * 8);
    let last = (n - 1) as f32;
    for j in 0..n {
        for i in 0..n {
            let [x, y, h] = at(i, j);
            let (l, r) = (at(i.saturating_sub(1), j), at(i + 1, j));
            let (u, d) = (at(i, j.saturating_sub(1)), at(i, j + 1));
            let gx = (r[2] - l[2]) / (r[0] - l[0]).abs().max(1e-4);
            let gy = (u[2] - d[2]) / (u[1] - d[1]).abs().max(1e-4);
            v.extend_from_slice(&[x, y, h, gx, gy, 0.0, i as f32 / last, j as f32 / last]);
        }
    }
    let mut idx = Vec::with_capacity((n - 1) * (n - 1) * 6 + 4 * (n - 1) * 6);
    for j in 0..n - 1 {
        for i in 0..n - 1 {
            let a = (j * n + i) as u32;
            let nn = n as u32;
            idx.extend_from_slice(&[a, a + 1, a + nn + 1, a, a + nn + 1, a + nn]);
        }
    }
    // Skirts: each edge hangs a short wall that hides cracks between leaves.
    let size = (pts[n - 1][0] - pts[0][0]).abs().max(1e-3);
    let drop = size * 0.06;
    let edges: [Vec<usize>; 4] = [
        (0..n).collect(),
        (0..n).map(|i| (n - 1) * n + i).collect(),
        (0..n).map(|j| j * n).collect(),
        (0..n).map(|j| j * n + n - 1).collect(),
    ];
    for e in edges {
        let base = (v.len() / 8) as u32;
        for &k in &e {
            let s = v[k * 8..k * 8 + 8].to_vec();
            v.extend_from_slice(&[s[0], s[1], s[2], s[3], s[4], drop, s[6], s[7]]);
        }
        for w in 0..e.len() - 1 {
            let (a, b) = (e[w] as u32, e[w + 1] as u32);
            let (la, lb) = (base + w as u32, base + w as u32 + 1);
            idx.extend_from_slice(&[a, b, lb, a, lb, la]);
        }
    }
    (v, idx)
}

fn bytes_f32(v: &[f32]) -> &[u8] {
    // SAFETY: f32 has no padding or invalid bit patterns as bytes.
    unsafe { std::slice::from_raw_parts(v.as_ptr().cast::<u8>(), std::mem::size_of_val(v)) }
}

fn bytes_u32(v: &[u32]) -> &[u8] {
    // SAFETY: as above.
    unsafe { std::slice::from_raw_parts(v.as_ptr().cast::<u8>(), std::mem::size_of_val(v)) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn leaf_mesh_has_skirts_and_valid_indices() {
        let n = LEAF_N;
        let pts: Vec<[f32; 3]> = (0..n * n)
            .map(|k| [(k % n) as f32, -((k / n) as f32), 0.1 * (k % n) as f32])
            .collect();
        let (v, idx) = leaf_mesh(&pts);
        let verts = v.len() / 8;
        assert_eq!(verts, n * n + 4 * n);
        assert!(idx.iter().all(|&i| (i as usize) < verts));
        // The slope: 0.1 km per km eastward.
        assert!((v[8 * (n + 3) + 3] - 0.1).abs() < 1e-4);
        // uv runs 0..1 across the leaf.
        assert_eq!((v[8 * (n * n - 1) + 6], v[8 * (n * n - 1) + 7]), (1.0, 1.0));
    }

    #[test]
    fn the_camera_projects_ahead_to_the_centre() {
        let f = Frame {
            eye: V3::new(0.0, -10.0, 0.0),
            right: V3::new(1.0, 0.0, 0.0),
            up: V3::new(0.0, 0.0, 1.0),
            fwd: V3::new(0.0, 1.0, 0.0),
            ex: 1.0,
            dark: true,
            min_dbz: 10.0,
            product: Product::Reflectivity,
            motion: [0.0; 2],
            terrain: true,
            radar: true,
            photo: false,
            leaves: Vec::new(),
            scene: Arc::new(test_scene()),
            scene_id: String::new(),
            haze: [0.0; 3],
        };
        let m = view_proj(&f, 1.0);
        // The origin is straight ahead: clip x, y = 0, w = 10.
        let (x, y, w) = (m[12], m[13], m[15]);
        assert!(
            x.abs() < 1e-4 && y.abs() < 1e-4 && (w - 10.0).abs() < 1e-3,
            "{m:?}"
        );
    }

    fn test_scene() -> Scene {
        Scene {
            lat: 0.0,
            lon: 0.0,
            half_km: 200.0,
            terrain: None,
            voxels: Vec::new(),
            voxel_km: 2.0,
            voxel_dz_km: 0.5,
            source: None,
            radar_xy: None,
            polar: None,
        }
    }
}
