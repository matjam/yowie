// GPU particle simulation. One step runs three kernels in one compute pass:
//
//   emit     moves the step's births into free slots taken from the dead
//            stack;
//   update   ages, moves and kills every slot, pushing dead slots back on
//            the stack and live ones onto the alive list;
//   finalize publishes the alive count as the indirect draw's instance
//            count and resets the accumulator for the next step.
//
// `slots` holds the dead stack in [0, capacity) and the alive list in
// [capacity, 2 * capacity). `header` holds the indirect draw arguments
// (vertex count, instance count, first vertex, first instance), then the
// dead stack height and the alive accumulator.

struct Particle {
    pos: vec2<f32>,
    vel: vec2<f32>,
    origin: vec2<f32>,
    age: f32,
    life: f32,
    size: f32,
    rot: f32,
    spin: f32,
    tint: u32,
};

struct Params {
    dt: f32,
    birth_first: u32,
    birth_count: u32,
    capacity: u32,
    gravity: vec2<f32>,
    drag: f32,
    radial: f32,
    tangential: f32,
    attractor_count: u32,
    pad0: u32,
    pad1: u32,
    // xy position, z strength, w radius.
    attractors: array<vec4<f32>, 8>,
};

struct Lut {
    offset: vec2<f32>,
    aspect: f32,
    frame_count: u32,
    capacity: u32,
    premult: u32,
    pad0: u32,
    pad1: u32,
    plane_origin: vec4<f32>,
    plane_x: vec4<f32>,
    plane_y: vec4<f32>,
    color: array<vec4<f32>, 128>,
    // x size multiplier, y speed multiplier, z alpha multiplier, w frame.
    curve: array<vec4<f32>, 128>,
    frames: array<vec4<f32>, 64>,
};

const DEAD: u32 = 4u;
const ALIVE: u32 = 5u;
const INSTANCES: u32 = 1u;

@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<storage, read_write> particles: array<Particle>;
@group(0) @binding(2) var<storage, read> births: array<Particle>;
@group(0) @binding(3) var<storage, read_write> slots: array<u32>;
@group(0) @binding(4) var<storage, read_write> header: array<atomic<u32>>;
@group(0) @binding(5) var<uniform> lut: Lut;

@compute @workgroup_size(64)
fn emit(@builtin(global_invocation_id) id: vec3<u32>) {
    if (id.x >= params.birth_count) {
        return;
    }
    let old = atomicSub(&header[DEAD], 1u);
    // An empty stack wraps to the top of the range: put the count back.
    if (old == 0u || old > params.capacity) {
        _ = atomicAdd(&header[DEAD], 1u);
        return;
    }
    let slot = slots[old - 1u];
    particles[slot] = births[params.birth_first + id.x];
}

fn speed_at(t: f32) -> f32 {
    let f = clamp(t, 0.0, 1.0) * 127.0;
    let i0 = u32(floor(f));
    let i1 = min(i0 + 1u, 127u);
    return mix(lut.curve[i0].y, lut.curve[i1].y, f - floor(f));
}

fn unit_or_zero(v: vec2<f32>) -> vec2<f32> {
    let l = length(v);
    return select(vec2<f32>(0.0), v / l, l > 1e-12);
}

@compute @workgroup_size(64)
fn update(@builtin(global_invocation_id) id: vec3<u32>) {
    let i = id.x;
    if (i >= params.capacity) {
        return;
    }
    var p = particles[i];
    if (p.life <= 0.0) {
        return;
    }
    let dt = params.dt;
    p.age += dt;
    if (p.age >= p.life) {
        particles[i].life = 0.0;
        let k = atomicAdd(&header[DEAD], 1u);
        slots[k] = i;
        return;
    }
    var accel = params.gravity;
    for (var a = 0u; a < params.attractor_count; a++) {
        let at = params.attractors[a];
        let d = at.xy - p.pos;
        let dist = length(d);
        if (at.w > 0.0 && dist < at.w && dist > 1e-4) {
            accel += d / dist * at.z * (1.0 - dist / at.w);
        }
    }
    if (params.radial != 0.0 || params.tangential != 0.0) {
        let r = unit_or_zero(p.pos - p.origin);
        accel += r * params.radial + vec2<f32>(-r.y, r.x) * params.tangential;
    }
    p.vel += accel * dt;
    if (params.drag > 0.0) {
        p.vel *= max(1.0 - params.drag * dt, 0.0);
    }
    let boost = speed_at(p.age / p.life);
    p.pos += p.vel * boost * dt;
    p.rot += p.spin * dt;
    particles[i] = p;
    let a = atomicAdd(&header[ALIVE], 1u);
    slots[params.capacity + a] = i;
}

@compute @workgroup_size(1)
fn finalize() {
    atomicStore(&header[INSTANCES], atomicLoad(&header[ALIVE]));
    atomicStore(&header[ALIVE], 0u);
}
