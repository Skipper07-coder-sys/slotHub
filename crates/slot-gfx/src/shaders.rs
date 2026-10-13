use crate::surface::GfxError;

const VERT_PREAMBLE: &str = "#version 330 core\n#define attribute in\n#define varying out\n";

const FRAG_PREAMBLE: &str = "#version 330 core\n#define varying in\n\
                             #define texture2D texture\nout vec4 FRAG_COLOR;\n";

const VERT_PREAMBLE_ES: &str = "";
const FRAG_PREAMBLE_ES: &str = "#define FRAG_COLOR gl_FragColor\n";

pub fn program(vert: &str, frag: &str) -> Result<gl::types::GLuint, GfxError> {
    let (vp, fp) = match crate::gl::es() {
        true => (VERT_PREAMBLE_ES, FRAG_PREAMBLE_ES),
        false => (VERT_PREAMBLE, FRAG_PREAMBLE),
    };
    crate::gl::program(&format!("{vp}{vert}"), &format!("{fp}{frag}"))
}

pub const RECT_VERT: &str = r#"
attribute vec2 a_pos;
uniform vec4 u_rect;
uniform vec2 u_target;
varying vec2 v_uv;
void main() {
    v_uv = a_pos;
    vec2 p = (u_rect.xy + a_pos * u_rect.zw) / u_target;
    gl_Position = vec4(p.x * 2.0 - 1.0, 1.0 - p.y * 2.0, 0.0, 1.0);
}
"#;

pub const SPRITE_VERT: &str = r#"
attribute vec2 a_pos;
uniform vec4 u_rect;
uniform vec2 u_target;
uniform vec2 u_turn;
varying vec2 v_uv;
void main() {
    v_uv = a_pos;
    vec2 mid = u_rect.zw * 0.5;
    vec2 local = a_pos * u_rect.zw - mid;
    vec2 turned = vec2(u_turn.x * local.x - u_turn.y * local.y,
                       u_turn.y * local.x + u_turn.x * local.y);
    vec2 p = (u_rect.xy + a_pos * u_rect.zw + (turned - local)) / u_target;
    gl_Position = vec4(p.x * 2.0 - 1.0, 1.0 - p.y * 2.0, 0.0, 1.0);
}
"#;

pub const GAME_FRAG: &str = r#"
#ifdef GL_FRAGMENT_PRECISION_HIGH
precision highp float;
#else
precision mediump float;
#endif
uniform sampler2D u_game;
uniform vec2 u_src;
uniform vec4 u_uv;
uniform float u_bright;
uniform float u_mode;
uniform vec4 u_rect;
uniform vec2 u_fbo;
uniform vec4 u_pic;
varying vec2 v_uv;
const vec3 offsets = vec3(3.141592654) * vec3(1.0 / 2.0, 1.0 / 2.0 - 2.0 / 3.0, 1.0 / 2.0 - 4.0 / 3.0);
vec2 grid_band(vec2 x) {
    return floor(x) / 3.0 + min(fract(x), vec2(1.0 / 3.0));
}
float dot_weight(vec2 f, vec2 o, float bloom) {
    return exp(-4.0 * length(f - (o + vec2(0.5))) * bloom);
}
// lcd-grid-v2: integrals of (1 - x^2 - x^4 + x^6)^2 and (1 - 2x^4 + x^6)^2.
vec3 smear_x(vec3 z) {
    vec3 z2 = z * z;
    return z * (1.0 + z2 * (-2.0 / 3.0 + z2 * (-1.0 / 5.0 + z2 * (4.0 / 7.0
        + z2 * (-1.0 / 9.0 + z2 * (-2.0 / 11.0 + z2 * (1.0 / 13.0)))))));
}
vec2 smear_y(vec2 z) {
    vec2 z2 = z * z;
    return z * (1.0 + z2 * (0.0 + z2 * (-4.0 / 5.0 + z2 * (2.0 / 7.0
        + z2 * (4.0 / 9.0 + z2 * (-4.0 / 11.0 + z2 * (1.0 / 13.0)))))));
}
vec3 smear_across(vec3 x, float dx) {
    vec3 lo = clamp((x - dx * 0.5) / 1.5, -1.0, 1.0);
    vec3 hi = clamp((x + dx * 0.5) / 1.5, -1.0, 1.0);
    return 1.5 * (smear_x(hi) - smear_x(lo)) / dx;
}
vec2 smear_down(vec2 y, float dy) {
    vec2 lo = clamp((y - dy * 0.5) / 0.63, -1.0, 1.0);
    vec2 hi = clamp((y + dy * 0.5) / 0.63, -1.0, 1.0);
    return 0.63 * (smear_y(hi) - smear_y(lo)) / dy;
}
// Gamma 2.0 rather than lcd-grid-v2's 2.2: squares and one square root instead of five pows,
// which the H700 could not keep up with. The 1.18 gain keeps the old brightness.
vec3 lcd_texel(vec2 at) {
    vec3 t = texture2D(u_game, (at + 0.5) / u_src).rgb;
    return t * t;
}
// zfast_crt_geo: NTSC-J (D93) to Rec709 D65, for colours squared on the way in.
const mat3 P22D93 = mat3(
    1.00000, 0.00000, -0.06173,
    0.07111, 0.96887, -0.01136,
    0.00000, 0.08197, 1.07280);
vec3 soft_texel(vec2 p) {
    vec2 t = p - 0.5;
    vec2 i = floor(t);
    vec2 w = t - i;
    vec2 a = (i + 0.5) / u_src;
    vec2 b = (i + 1.5) / u_src;
    vec3 top = mix(texture2D(u_game, a).rgb, texture2D(u_game, vec2(b.x, a.y)).rgb, w.x);
    vec3 bottom = mix(texture2D(u_game, vec2(a.x, b.y)).rgb, texture2D(u_game, b).rgb, w.x);
    return mix(top, bottom, w.y);
}
void main() {
    vec2 uv = u_uv.xy + v_uv * u_uv.zw;
    vec2 f = fract(uv * u_src);
    vec3 c = texture2D(u_game, uv).rgb;
    bool inside = all(greaterThanEqual(uv, u_pic.xy)) && all(lessThan(uv, u_pic.zw));
    vec2 ps = u_uv.zw * u_src / (u_rect.zw * u_fbo);
    if (!inside) {
    } else if (u_mode > 9.5) {
        // LCD3x 18/12: LCD3x with brighter scanlines (18) and LCD (12).
        vec2 angle = f * 6.283185307;
        c *= (18.0 + sin(angle.y)) / 19.0 * (12.0 + sin(angle.x + offsets)) / 13.0;
    } else if (u_mode > 8.5) {
        vec2 span = u_pic.zw - u_pic.xy;
        vec2 q = (uv - u_pic.xy) / span;
        vec2 w = q * 2.0 - 1.0;
        w *= vec2(1.0 + w.y * w.y * 0.0276, 1.0 + w.x * w.x * 0.0414);
        w = w * 0.5 + 0.5;
        vec2 corner = min(w, 1.0 - w);
        corner.x = 0.0001 / corner.x;
        vec2 edge = q * (1.0 - q);
        float vig = min(sqrt(edge.x * edge.y * 46.0), 1.0);
        vec2 t = (u_pic.xy + w * span) * u_src;
        float row = floor(t.y - 0.5) + 0.5;
        float fy = t.y - 0.5 - row;
        float y2 = fy * fy;
        c = soft_texel(vec2(t.x, row + 0.5 + 4.0 * y2 * fy));
        float mask = 1.0 - 0.5 * step(0.5, fract(floor(gl_FragCoord.x) * 0.5));
        c = max((c * c) * (P22D93 * vig), 0.0);
        float scan = 1.5 - 7.0 * (y2 - y2 * y2);
        c *= mix(scan * mask, 1.0, dot(c, vec3(0.26667)));
        vec3 power = vec3(1.0 / ((-0.0325 * 7.0 + 1.0) * (-0.311 * 0.5 + 1.0)) - 1.2);
        vec3 cir = (c - 1.0) * (c - 1.0);
        c = mix(sqrt(c), sqrt(max(1.0 - cir, 0.0)), power);
        if (corner.y <= corner.x || corner.x < 0.0001) {
            c = vec3(0.0);
        }
    } else if (u_mode > 7.5) {
        vec2 tl = floor(uv * u_src - 0.4999);
        float sx = (uv.x * u_src.x - 0.4999 - tl.x) * 3.0;
        float rx = ps.x * 3.0;
        vec3 lcol = smear_across(vec3(sx + 1.0, sx, sx - 1.0), rx);
        vec3 rcol = smear_across(vec3(sx - 2.0, sx - 3.0, sx - 4.0), rx);
        float sy = uv.y * u_src.y - 0.4999 - tl.y;
        vec2 tb = smear_down(vec2(sy, sy - 1.0), ps.y);
        float tcol = tb.x;
        float bcol = tb.y;
        vec3 sum = lcd_texel(tl) * lcol * tcol
                 + lcd_texel(tl + vec2(1.0, 0.0)) * rcol * tcol
                 + lcd_texel(tl + vec2(0.0, 1.0)) * lcol * bcol
                 + lcd_texel(tl + vec2(1.0, 1.0)) * rcol * bcol;
        c = 1.18 * sqrt(sum);
    } else if (u_mode > 6.5) {
        vec2 p = uv * u_src;
        vec2 i = floor(p) + 0.5;
        vec2 d = p - i;
        p = i + 4.0 * d * d * d;
        p.x = mix(p.x, uv.x * u_src.x, 0.45);
        c = soft_texel(p);
        float y2 = d.y * d.y;
        float y4 = y2 * y2;
        float mask = 1.0 - 0.25 * step(0.5, fract(floor(gl_FragCoord.x) * 0.5));
        float low = 1.25 - 5.0 * (y2 - 2.05 * y4);
        float high = 1.0 - 10.0 * (y4 - 2.8 * y4 * d.y);
        c *= mix(low * mask, high, dot(c, vec3(0.3333 * 0.8)));
    } else if (u_mode > 5.5) {
        c = pow(c, vec3(2.4));
        vec3 lift = mix(vec3(0.2), vec3(0.8), c) * (1.0 - sqrt(f.x + f.y));
        c = pow(clamp(c + lift, 0.0, 1.0), vec3(1.0 / 2.2));
    } else if (u_mode > 4.5) {
        c = pow(c, vec3(2.4));
        vec2 gap = clamp(clamp(f + 0.5 * ps, 0.0, 1.0) - 0.84, vec2(0.0), ps) / ps;
        c = mix(c * (1.04 + f.x * f.y), c * 0.36, max(gap.x, gap.y));
        c = clamp(pow(c, vec3(1.0 / 2.2)), 0.0, 1.0);
    } else if (u_mode > 3.5) {
        vec2 angle = (f - 0.25) * 6.283185307;
        c *= (16.0 + sin(angle.y)) / 17.0 * (4.0 + sin(angle.x)) / 5.0;
    } else if (u_mode > 2.5) {
        float bloom = mix(1.05, 0.95, dot(c, vec3(0.30, 0.59, 0.11)));
        float mid = dot_weight(f, vec2(0.0), bloom);
        float sum = 0.0;
        for (int j = -1; j <= 1; j++) {
            for (int i = -1; i <= 1; i++) {
                sum += dot_weight(f, vec2(float(i), float(j)), bloom);
            }
        }
        c = mix(c, mix(1.2 * c * mid, c * sum, 0.85), 0.85);
    } else if (u_mode > 1.5) {
        vec2 hp = 0.5 * ps;
        vec2 edge = (grid_band(f + hp) - grid_band(f - hp)) / (2.0 * hp);
        c *= 1.0 - 0.75 * max(edge.x, edge.y);
    } else if (u_mode > 0.5) {
        vec2 angle = f * 6.283185307;
        float yfactor = (16.0 + sin(angle.y)) / 17.0;
        vec3 xfactors = (4.0 + sin(angle.x + offsets)) / 5.0;
        c *= yfactor * xfactors;
    }
    FRAG_COLOR = vec4(c * u_bright, 1.0);
}
"#;

pub const SPRITE_FRAG: &str = r#"
precision mediump float;
uniform sampler2D u_tex;
uniform vec4 u_colour;
varying vec2 v_uv;
void main() {
    FRAG_COLOR = texture2D(u_tex, v_uv) * u_colour;
}
"#;

pub const BLIT_VERT: &str = r#"
attribute vec2 a_pos;
uniform vec4 u_uv;
varying vec2 v_uv;
void main() {
    v_uv = u_uv.xy + a_pos * u_uv.zw;
    gl_Position = vec4(a_pos * 2.0 - 1.0, 0.0, 1.0);
}
"#;

pub const TURN_VERT: &str = r#"
attribute vec2 a_pos;
uniform float u_ccw;
varying vec2 v_uv;
void main() {
    vec2 cw = vec2(1.0 - a_pos.y, a_pos.x);
    vec2 ccw = vec2(a_pos.y, 1.0 - a_pos.x);
    v_uv = mix(cw, ccw, u_ccw);
    gl_Position = vec4(a_pos * 2.0 - 1.0, 0.0, 1.0);
}
"#;

pub const BLIT_FRAG: &str = r#"
precision mediump float;
uniform sampler2D u_tex;
uniform vec3 u_gain;
varying vec2 v_uv;
void main() {
    FRAG_COLOR = vec4(texture2D(u_tex, v_uv).rgb * u_gain, 1.0);
}
"#;

pub const BEZEL_FRAG: &str = r#"
precision mediump float;
uniform sampler2D u_tex;
uniform vec3 u_gain;
uniform float u_alpha;
varying vec2 v_uv;
void main() {
    vec4 c = texture2D(u_tex, v_uv);
    FRAG_COLOR = vec4(c.rgb * u_gain, c.a * u_alpha);
}
"#;
