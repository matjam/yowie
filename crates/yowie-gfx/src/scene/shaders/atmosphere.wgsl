// The procedural sky: single scattering through an atmosphere of air
// (Rayleigh) and haze (Mie) thinning with height, or a three-colour
// gradient when there is no atmosphere. `sky.rs` holds the same model on
// the processor for the sun's colour at the ground.

// Where a ray from o along d crosses a sphere of radius r about the
// origin, as two distances; x is greater than y when it misses.
fn ray_sphere(o: vec3<f32>, d: vec3<f32>, r: f32) -> vec2<f32> {
    let b = dot(o, d);
    let c = dot(o, o) - r * r;
    var h = b * b - c;
    if h < 0.0 {
        return vec2<f32>(1.0, -1.0);
    }
    h = sqrt(h);
    return vec2<f32>(-b - h, -b + h);
}

fn phase_rayleigh(mu: f32) -> f32 {
    return 3.0 / (16.0 * PI) * (1.0 + mu * mu);
}

fn phase_mie(mu: f32, g: f32) -> f32 {
    let g2 = g * g;
    let d = (2.0 + g2) * pow(max(1.0 + g2 - 2.0 * g * mu, 1e-4), 1.5);
    return 3.0 / (8.0 * PI) * ((1.0 - g2) * (1.0 + mu * mu)) / d;
}

struct Scatter {
    radiance: vec3<f32>,
    transmittance: vec3<f32>,
}

// Integrates single scattering along a ray leaving the camera in
// direction d for at most dist world units. Each sample is lit by the
// sunlight left after the air towards the sun and dimmed by the air back
// to the camera; samples in the planet's shadow are dark.
fn atmosphere_scatter(d: vec3<f32>, dist: f32, steps: i32, sun_steps: i32) -> Scatter {
    let radius = frame.atmos.x;
    let height = frame.atmos.y;
    let h_r = frame.atmos.z;
    let h_m = frame.atmos.w;
    let beta_r = frame.beta_r.rgb;
    let beta_m = frame.beta_m.x;
    let sun = frame.sun_dir.xyz;
    let origin = frame.sky_up.xyz * (radius + frame.beta_m.z);
    var out = Scatter(vec3<f32>(0.0), vec3<f32>(1.0));
    let shell = ray_sphere(origin, d, radius + height);
    let t0 = max(shell.x, 0.0);
    var t1 = shell.y;
    if t1 <= t0 {
        return out;
    }
    let ground = ray_sphere(origin, d, radius);
    if ground.y > 0.0 && ground.x > 0.0 {
        t1 = min(t1, ground.x);
    }
    t1 = min(t1, t0 + dist);
    if t1 <= t0 {
        return out;
    }
    let ds = (t1 - t0) / f32(steps);
    let mu = dot(d, sun);
    var od_r = 0.0;
    var od_m = 0.0;
    var sum_r = vec3<f32>(0.0);
    var sum_m = vec3<f32>(0.0);
    for (var i = 0; i < steps; i++) {
        let p = origin + d * (t0 + (f32(i) + 0.5) * ds);
        let h = max(length(p) - radius, 0.0);
        let d_r = exp(-h / h_r) * ds;
        let d_m = exp(-h / h_m) * ds;
        // The light a step scatters is dimmed by the air before it and,
        // on average, by its own air: (1 - e^-x) / x of it survives. That
        // keeps long steps near the horizon from losing all their blue.
        let seg = beta_r * d_r + beta_m * 1.1 * d_m;
        let own = (vec3<f32>(1.0) - exp(-seg)) / max(seg, vec3<f32>(1e-6));
        let od_r_before = od_r;
        let od_m_before = od_m;
        od_r += d_r;
        od_m += d_m;
        let shadow = ray_sphere(p, sun, radius);
        if shadow.y > 0.0 && shadow.x > 0.0 {
            continue;
        }
        let light_step = max(ray_sphere(p, sun, radius + height).y, 0.0) / f32(sun_steps);
        var lod_r = 0.0;
        var lod_m = 0.0;
        for (var j = 0; j < sun_steps; j++) {
            let q = p + sun * ((f32(j) + 0.5) * light_step);
            let hj = max(length(q) - radius, 0.0);
            lod_r += exp(-hj / h_r) * light_step;
            lod_m += exp(-hj / h_m) * light_step;
        }
        let att = exp(-(beta_r * (od_r_before + lod_r) + beta_m * 1.1 * (od_m_before + lod_m))) * own;
        sum_r += att * d_r;
        sum_m += att * d_m;
    }
    out.transmittance = exp(-(beta_r * od_r + beta_m * 1.1 * od_m));
    out.radiance = frame.beta_r.w * (sum_r * beta_r * phase_rayleigh(mu) + sum_m * beta_m * phase_mie(mu, frame.beta_m.y));
    return out;
}

// The sky's light from direction d without the sun's disc: the
// atmosphere when there is one, otherwise the gradient, and the ground
// below the horizon.
fn sky_color(d: vec3<f32>) -> vec3<f32> {
    let up_axis = frame.sky_up.xyz;
    let up = dot(d, up_axis);
    let air = frame.sky_horizon.w;
    if frame.beta_m.w > 0.5 {
        var dir = d;
        // Inside the air, a ray below the horizon meets the ground at
        // once; look the colour up along the horizon instead.
        if up < 0.0 && frame.beta_m.z < frame.atmos.y {
            let side = d - up_axis * up;
            let len = length(side);
            if len > 1e-4 {
                dir = side / len;
            }
        }
        var c = atmosphere_scatter(dir, 1e9, 8, 4).radiance * air;
        if up < 0.0 {
            c = mix(c, frame.sky_ground.rgb, pow(-up, 0.5));
        }
        return c;
    }
    let above = mix(frame.sky_horizon.rgb, frame.sky_zenith.rgb, pow(clamp(up, 0.0, 1.0), 0.7)) * air;
    let below = mix(frame.sky_horizon.rgb * air, frame.sky_ground.rgb, pow(clamp(-up, 0.0, 1.0), 0.5));
    return select(below, above, up >= 0.0);
}

fn star_hash(point: vec3<f32>) -> f32 {
    var p = fract(point * 0.3183099 + vec3<f32>(0.1, 0.2, 0.3));
    p *= 17.0;
    return fract(p.x * p.y * p.z * (p.x + p.y + p.z));
}

// A fixed field of stars, one in about every sixtieth cell of a grid over
// the direction.
fn stars(d: vec3<f32>) -> vec3<f32> {
    let p = d * 90.0;
    let cell = floor(p);
    let h = star_hash(cell);
    if h < 0.985 {
        return vec3<f32>(0.0);
    }
    let centre = cell + 0.5 + (vec3<f32>(star_hash(cell + 1.0), star_hash(cell + 2.0), star_hash(cell + 3.0)) - 0.5) * 0.6;
    let glow = smoothstep(0.14, 0.0, length(p - centre));
    let bright = 0.4 + 3.0 * fract(h * 13.0);
    let tint = mix(vec3<f32>(0.8, 0.9, 1.0), vec3<f32>(1.0, 0.9, 0.75), fract(h * 7.0));
    return tint * glow * bright;
}

// The full sky in direction d: the sky colour, the sun's disc and glow,
// and stars where the air is thin.
fn sky_with_sun(d: vec3<f32>) -> vec3<f32> {
    var color = sky_color(d);
    let up = dot(d, frame.sky_up.xyz);
    let air = frame.sky_horizon.w;
    if frame.sun_dir.w > 0.5 {
        let sun = frame.sun_dir.xyz;
        let r = frame.sun_disc.w;
        let c = dot(d, sun);
        let visible = smoothstep(-r, r, dot(sun, frame.sky_up.xyz)) * step(0.0, up);
        let disc = smoothstep(cos(r * 1.25), cos(r), c);
        var tint = vec3<f32>(1.0);
        if frame.beta_m.w > 0.5 {
            tint = atmosphere_scatter(sun, 1e9, 4, 1).transmittance;
        }
        color += frame.sun_disc.rgb * tint * disc * visible;
        color += frame.sun_disc.rgb * tint * air * 0.002 * pow(max(c, 0.0), 32.0) * visible;
    }
    let night = frame.sky_up.w * (1.0 - air * 0.9) * smoothstep(-0.01, 0.01, up);
    if night > 0.0 {
        color += stars(d) * night;
    }
    return color;
}
