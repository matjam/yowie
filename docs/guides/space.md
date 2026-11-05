---
title: Orbits and space
group: Simulation
order: 2
summary: celestial mechanics for any star system, Kepler orbits, transfers, ships under thrust, time warp and N-body gravity
---

# Orbits and space

`yowie::orbit` computes celestial mechanics. It turns orbital elements into
positions and back, moves bodies exactly along their orbits, plans transfers,
runs star systems where ships fly under gravity and their own thrust, and
integrates N-body gravity. The module needs no window or device, so its
functions run in tests and on servers. Real data for our solar system is in
`yowie::orbit::sol`.

To see it in a game, run the space example:

```bash
cargo run --example space
```

For every type and function, run `cargo doc --open` and open `yowie::orbit`.

## Units and conventions

All quantities are `f64`. Positions and velocities are `DVec3`, re-exported as
`yowie::orbit::DVec3`. The crate fixes no units. Pick a length, a mass and a
time unit, and use them everywhere, including in the gravitational constant.

| Convention | Value |
|---|---|
| Number type | `f64`; vectors are `DVec3` |
| Angles | radians |
| Reference plane | XY, with +Z as north |
| Angle origin | +X, the reference longitude |
| Gravitational parameter `mu` | `g * mass` of the primary |
| `yowie::orbit::G` | 6.67430e-11 m³ kg⁻¹ s⁻² |
| `sol` module | metres, kilograms and seconds |

Functions that compute an orbit around one primary take its `mu` rather than
its mass. For our solar system, use metres, kilograms and seconds with `G` and
the `sol` constants. For an invented system, set `g` to 1 and choose masses and
distances that give the orbit periods you want. One lap of a circular orbit of
radius `r` takes `2π sqrt(r³ / mu)` time units.

```rust
use yowie::orbit::{Elements, G, sol};

fn main() {
    // Our solar system in SI units.
    let earth = Elements::circular(sol::AU);
    let year = earth.period(G * sol::SUN.mass).unwrap_or(0.0);
    println!("one orbit at 1 AU takes {:.1} days", year / sol::DAY);

    // A game system with g = 1: a star of mass 175 and a planet 40 units out.
    let lap = Elements::circular(40.0).period(1.0 * 175.0).unwrap_or(0.0);
    assert!((lap - 120.2).abs() < 0.1); // two minutes of game time
}
```

## Orbits from elements

`Elements` holds the six classical orbital elements of an orbit around one
primary.

| Field | Meaning |
|---|---|
| `semi_major_axis` | Half the long axis. Negative for a hyperbola. For a parabola it holds the periapsis distance. |
| `eccentricity` | 0 circular, below 1 elliptic, 1 parabolic, above 1 hyperbolic |
| `inclination` | Tilt of the orbital plane from the XY plane, in `[0, π]` |
| `node` | Longitude of the ascending node, measured from +X |
| `arg_periapsis` | Angle from the ascending node to periapsis |
| `true_anomaly` | Angle from periapsis to the body, which sets where on the orbit the body is |

To make a circular orbit in the XY plane that starts on +X, call
`Elements::circular(radius)`. For any other orbit, fill in the fields you need
and take the rest from `Elements::default()`, which is all zeros.

To get a position and velocity, call `Elements::state(mu)`. It returns a
`State { pos, vel }` relative to the primary. Add the primary's own state to
place the body in a larger frame.

`Elements` also computes the figures an orbit readout shows:

| Method | Returns |
|---|---|
| `periapsis()` | Closest distance to the primary |
| `apoapsis()` | Farthest distance, or `None` for an open orbit |
| `period(mu)` | Time for one orbit, or `None` for an open orbit |
| `mean_motion(mu)` | Average angular rate, `sqrt(mu / abs(a)³)` radians per time unit |
| `mean_anomaly()` | Mean anomaly, or the hyperbolic mean anomaly when `eccentricity > 1` |
| `is_closed()` | `true` for a circle or an ellipse |
| `is_parabolic()` | `true` when the eccentricity is within 1e-9 of 1 |

```rust
use yowie::orbit::Elements;

/// One line of an orbit readout around a primary with parameter `mu`.
fn describe(el: &Elements, mu: f64) -> String {
    match (el.apoapsis(), el.period(mu)) {
        (Some(apo), Some(period)) => format!(
            "periapsis {:.1}, apoapsis {apo:.1}, period {period:.0}",
            el.periapsis()
        ),
        _ => format!("escaping, periapsis {:.1}", el.periapsis()),
    }
}

fn main() {
    let planet = Elements {
        semi_major_axis: 70.0,
        eccentricity: 0.05,
        inclination: 0.03,
        node: 2.0,
        arg_periapsis: 0.4,
        true_anomaly: 0.0,
    };
    println!("{}", describe(&planet, 175.0));
    println!("starts at {:?}", planet.state(175.0).pos);
}
```

## Orbits from position and velocity

To find the orbit a body is on, call `Elements::from_state(state, mu)` with its
state relative to the primary. When both states are in a larger frame, subtract
the primary's position and velocity first.

Angles that an orbit leaves undefined are set to 0: the node of an orbit in the
XY plane, and the argument of periapsis of a circular orbit. The remaining
angles are then measured from +X, so the result converts back to the same
state.

```rust
use yowie::orbit::{DVec3, Elements, State};

/// The orbit of a ship around a planet, from both states in the same frame.
fn orbit_around(ship: State, planet: State, mu: f64) -> Elements {
    let rel = State {
        pos: ship.pos - planet.pos,
        vel: ship.vel - planet.vel,
    };
    Elements::from_state(rel, mu)
}

fn main() {
    let mu = 1.2;
    let planet = State {
        pos: DVec3::new(110.0, 0.0, 0.0),
        vel: DVec3::new(0.0, 1.26, 0.0),
    };
    let low = Elements::circular(2.4).state(mu);
    let ship = State {
        pos: planet.pos + low.pos,
        vel: planet.vel + low.vel,
    };
    let el = orbit_around(ship, planet, mu);
    assert!((el.semi_major_axis - 2.4).abs() < 1e-9);
}
```

When a `System` holds the bodies, `System::around` finds the primary and the
orbit in one call. See [Predicting a path](#predicting-a-path).

## Moving along an orbit

To advance a state along its orbit, call `propagate(state, mu, dt)`. It solves
the two-body problem exactly with universal variables, for circles, ellipses,
parabolas and hyperbolas, and `dt` may be negative. One call covers any span,
so a moon's position a year ahead costs the same as its position a second
ahead.

To advance elements instead, call `Elements::at_time(mu, dt)`. It returns the
same orbit with only `true_anomaly` changed. For a hyperbola the true anomaly
comes back in `[0, 2π)`; a value above π means the body has not yet reached
periapsis.

```rust
use std::f64::consts::PI;
use yowie::orbit::{Elements, propagate};

fn main() {
    let mu = 175.0;
    let el = Elements {
        semi_major_axis: 40.0,
        eccentricity: 0.15,
        ..Elements::default()
    };
    let half = el.period(mu).unwrap_or(0.0) / 2.0;

    // Half a period after periapsis, the body is at apoapsis.
    let s = propagate(el.state(mu), mu, half);
    let apo = el.apoapsis().unwrap_or(0.0);
    assert!((s.pos.length() - apo).abs() < 1e-6 * apo);

    // The same step taken on the elements.
    assert!((el.at_time(mu, half).true_anomaly - PI).abs() < 1e-9);
}
```

The Kepler equation solvers behind these are public, for code that works with
anomalies directly:

| Function | Returns |
|---|---|
| `solve_kepler(m, e)` | Eccentric anomaly from mean anomaly, for `0 <= e < 1`, in `[0, 2π)` |
| `solve_kepler_hyperbolic(m, e)` | Hyperbolic anomaly from hyperbolic mean anomaly, for `e > 1` |
| `true_anomaly_from_mean(m, e)` | True anomaly in `[0, 2π)` on an ellipse |
| `mean_anomaly_from_true(nu, e)` | Mean anomaly in `[0, 2π)` on an ellipse |
| `true_anomaly_from_mean_hyperbolic(m, e)` | True anomaly in `(-π, π)` on a hyperbola, negative on the way in |
| `mean_anomaly_from_true_hyperbolic(nu, e)` | Hyperbolic mean anomaly; `nu` must lie inside the asymptotes, `abs(nu) < acos(-1 / e)` |
| `wrap_angle(a)` | `a` wrapped into `[0, 2π)` |

## Speeds, energy and transfers

These functions compute the figures a flight readout or a mission planner
needs. Each takes the primary's `mu`, except the last two, which take masses.

| Function | Returns |
|---|---|
| `circular_velocity(mu, r)` | Speed of a circular orbit of radius `r` |
| `escape_velocity(mu, r)` | Speed that escapes from radius `r` on a parabola |
| `vis_viva(mu, r, a)` | Speed at distance `r` on an orbit with semi-major axis `a` (negative for a hyperbola) |
| `energy(state, mu)` | Specific orbital energy, negative for a closed orbit |
| `hohmann(mu, r1, r2)` | A `Hohmann` transfer between circular orbits of radii `r1` and `r2` |
| `sphere_of_influence(a, m, big_m)` | `a (m / M)^(2/5)`, the radius inside which the body of mass `m` is the better primary |
| `hill_radius(a, e, m, big_m)` | `a (1 - e) (m / 3M)^(1/3)`; moons stay in orbit only well inside it |

A `Hohmann` has three fields. `dv1` is the speed change that leaves the first
orbit, `dv2` the speed change that circularises at the second, and `coast` the
time between them, which is half the transfer ellipse's period. Both speed
changes are magnitudes. Burn prograde for both when `r2 > r1`, and retrograde
for both when `r2 < r1`.

```rust
use yowie::orbit::{hohmann, sol};

fn main() {
    // From Earth's orbit to Mars's orbit, both treated as circles.
    let r1 = sol::EARTH.orbit.map_or(sol::AU, |o| o.semi_major_axis);
    let r2 = sol::MARS.orbit.map_or(1.5 * sol::AU, |o| o.semi_major_axis);
    let plan = hohmann(sol::MU_SUN, r1, r2);
    println!(
        "{:.2} km/s, coast {:.0} days, {:.2} km/s",
        plan.dv1 / 1e3,
        plan.coast / sol::DAY,
        plan.dv2 / 1e3
    );
}
```

## Defining a star system

A `System` holds a star system for a game. It has three kinds of member:

- Fixed bodies, added with `add_fixed(mass, state)`, move in a straight line at
  constant velocity from their starting state. Use one for a star, or for a
  body whose path the game sets itself with `set_fixed_state`.
- Orbiting bodies, added with `add_orbiting(mass, primary, elements)`, follow
  an exact Kepler orbit around another body. Planets and moons go here. Their
  positions are computed from the orbit and the system's time, so they do not
  drift however long the game runs or however fast time is warped.
- Ships, added with `add_ship(state)`, are integrated numerically under the
  gravity of every body with mass plus their own thrust. Ships exert no
  gravity, on bodies or on each other.

Bodies do not pull on each other. A planet follows its orbit whatever its
neighbours weigh, and only ships feel every body. For mutual gravity between
all bodies, use [`Simulation`](#n-body-gravity).

To create a system, call `System::new(g)` with your gravitational constant.
`System::default()` uses `G` for SI units. Set `g` before adding bodies,
because `add_orbiting` reads `g * primary mass` once, when the body is added.

`add_orbiting` returns `Result<BodyId, OrbitError>`. It fails with
`OrbitError::UnknownBody` when `primary` is not in the system, so add each
primary before its moons. The elements describe the orbit at the system's
time 0, not at the current time. To add a body after the game has started, at
the place its elements describe now, pass `elements.at_time(mu, -sys.time())`.

`System` stores no names. Keep your own table from `BodyId` to a name, a
radius and a colour, as the space example's `Look` struct does.

```rust
use yowie::orbit::{BodyId, Elements, OrbitError, State, System};

/// A body and the name the game shows for it.
struct Named {
    name: &'static str,
    id: BodyId,
}

/// A star, two planets and a moon, in game units with g = 1.
fn build() -> Result<(System, Vec<Named>), OrbitError> {
    let mut sys = System::new(1.0);
    let star = sys.add_fixed(175.0, State::default());
    let ferro = sys.add_orbiting(
        0.8,
        star,
        Elements {
            semi_major_axis: 70.0,
            eccentricity: 0.05,
            node: 2.0,
            arg_periapsis: 0.4,
            ..Elements::default()
        },
    )?;
    let vessa = sys.add_orbiting(
        1.2,
        star,
        Elements {
            semi_major_axis: 110.0,
            eccentricity: 0.03,
            true_anomaly: 2.5,
            ..Elements::default()
        },
    )?;
    let moon = sys.add_orbiting(0.01, vessa, Elements::circular(5.0))?;
    let names = vec![
        Named { name: "Kaal", id: star },
        Named { name: "Ferro", id: ferro },
        Named { name: "Vessa", id: vessa },
        Named { name: "Vessa I", id: moon },
    ];
    Ok((sys, names))
}

fn main() -> Result<(), OrbitError> {
    let (mut sys, names) = build()?;
    sys.step(10.0);
    for body in &names {
        let pos = sys.body_state(body.id).map(|s| s.pos).unwrap_or_default();
        println!("{} at {pos:?}", body.name);
    }
    Ok(())
}
```

To read a body, call `body_state(id)` for its position and velocity in the
system's frame, and `body_mass(id)` for its mass. Both return `None` for an id
the system did not hand out. Ids are indices into one system, so use them only
with the system that returned them. `body_ids()` lists every body in the order
it was added.
`time()` returns the time elapsed since the system was created.

A body with zero mass follows its orbit and pulls on nothing, as the comet in
the space example does. It still costs a Kepler solve every time the system
places its bodies, which happens several times per ship integration step. For
decoration such as an asteroid belt, keep the bodies out of the system and
place each one with `Elements::at_time(mu, sys.time())` when you draw it. The
space example draws its 300 belt rocks this way.

## Ships under thrust

To add a ship, call `add_ship(state)` with its position and velocity in the
system's frame. It returns a `ShipId`. To put a ship in orbit around a planet,
add the orbit's state to the planet's:

```rust
use yowie::orbit::{BodyId, Elements, ShipId, State, System};

/// Puts a ship on a circular orbit of `radius` around `planet`.
fn launch(sys: &mut System, planet: BodyId, radius: f64) -> Option<ShipId> {
    let at = sys.body_state(planet)?;
    let mu = sys.g * sys.body_mass(planet)?;
    let orbit = Elements::circular(radius).state(mu);
    Some(sys.add_ship(State {
        pos: at.pos + orbit.pos,
        vel: at.vel + orbit.vel,
    }))
}
```

To fire the engine, call `set_thrust(id, accel)`. The thrust is an
acceleration in the system's frame. Ships have no mass, so give the
acceleration the engine produces. The thrust stays on until you set it again;
set it to `DVec3::ZERO` to cut the engine. Because the vector is fixed in the
system's frame, recompute a burn along the ship's velocity on every update as
the velocity turns:

```rust
use yowie::orbit::{DVec3, ShipId, System};

/// Thrusts along the ship's velocity at `accel`. Pass 0 to cut the engine.
fn burn_prograde(sys: &mut System, ship: ShipId, accel: f64) {
    let Some(s) = sys.ship(ship) else { return };
    let dir = s.state.vel.normalize_or(DVec3::X);
    let _ = sys.set_thrust(ship, dir * accel);
}
```

`set_thrust` returns `OrbitError::UnknownShip` for an id the system did not
hand out.
To read a ship, call `ship(id)`, which returns a `&Ship` with its `state` and
`thrust`. To change either directly, call `ship_mut(id)`.

The space example builds three directions from the ship's velocity: prograde,
sideways (prograde crossed with +Z) and out of the orbital plane. W and S
thrust along the first, A and D along the second, Q and E along the third.

To advance the system, call `step(dt)`. Ships fly first, under gravity and
thrust, and then the bodies move to their places at the new time. `step`
ignores a `dt` that is zero, negative or not finite.

Ships are integrated with fourth-order Runge-Kutta, with the bodies placed at
each stage's time. Each `step` takes at least `substeps` integration steps per
ship (default 8). Where a ship's acceleration is large compared with its
speed, as in a tight orbit or a hard burn from rest, the integrator shortens
its steps to at most `0.01 × speed / acceleration`, which is about 630 steps
per circular orbit. It never shortens them below 1/64 of `dt / substeps`. The
cost of one `step` grows with the number of ships, the number of integration
steps and the number of bodies.

`softening` adds a distance in quadrature to every separation in the gravity
ships feel, which keeps a pass through a body's centre finite. It defaults
to 0.

## Burns and transfers

To change a ship's velocity in an instant, edit its state through `ship_mut`.
Combined with `hohmann`, this moves a ship between circular orbits: burn
`dv1`, coast for `coast`, burn `dv2`.

```rust
use yowie::orbit::{DVec3, Elements, ShipId, State, System, hohmann};

/// Adds `dv` to the ship's speed along its velocity. Negative slows it.
fn kick(sys: &mut System, ship: ShipId, dv: f64) {
    if let Some(s) = sys.ship_mut(ship) {
        let dir = s.state.vel.normalize_or(DVec3::X);
        s.state.vel += dir * dv;
    }
}

/// Advances `sys` by `span` in steps of at most 0.25 time units.
fn coast(sys: &mut System, span: f64) {
    let steps = (span / 0.25).ceil().max(1.0);
    for _ in 0..steps as u32 {
        sys.step(span / steps);
    }
}

fn main() {
    let mu = 175.0;
    let mut sys = System::new(1.0);
    sys.add_fixed(mu, State::default());
    let ship = sys.add_ship(Elements::circular(40.0).state(mu));

    let plan = hohmann(mu, 40.0, 70.0);
    kick(&mut sys, ship, plan.dv1);
    coast(&mut sys, plan.coast);
    kick(&mut sys, ship, plan.dv2);

    // The star sits at the origin, so the ship's state is already relative to it.
    let state = sys.ship(ship).map(|s| s.state).unwrap_or_default();
    let orbit = Elements::from_state(state, mu);
    assert!((orbit.semi_major_axis - 70.0).abs() < 0.1);
    assert!(orbit.eccentricity < 1e-3);
}
```

For a burn the player holds down, use `set_thrust` instead. A change of speed
`dv` at acceleration `a` takes `dv / a`. To show the result of a planned burn
before the player commits to it, predict the changed state with
`predict_state`, described next.

## Predicting a path

To draw where a ship is going, call `predict(ship, duration, samples,
reference)`. It integrates a copy of the ship over the next `duration` with
its current thrust held constant, and returns its position at `samples`
equally spaced times, the last at `duration`. The ship itself does not move.
The bodies move along their orbits during the prediction.

With `reference` set to `None`, the positions are in the system's frame, and a
path around a moving planet draws as a spiral. With `Some(body)`, each sample
has the body's movement since now subtracted, so a path around a planet closes
into an orbit drawn around where the planet is now.

To choose the reference, call `around(state)`. It returns the body whose
gravity dominates at that state (the largest `mass / distance²`), the orbit of
the state around that body as `Elements`, and the body's `mu`. It returns
`None` when no body has mass.

```rust
use yowie::orbit::{DVec3, ShipId, System};

/// The ship's path for the next orbit and a half around whichever body
/// dominates it, or a fixed span while it escapes.
fn path_ahead(sys: &System, ship: ShipId) -> Vec<DVec3> {
    let Some(state) = sys.ship(ship).map(|s| s.state) else {
        return Vec::new();
    };
    let Some((primary, orbit, mu)) = sys.around(state) else {
        return Vec::new();
    };
    let horizon = orbit.period(mu).map_or(60.0, |p| (1.5 * p).min(600.0));
    sys.predict(ship, horizon, 90, Some(primary))
        .unwrap_or_default()
}
```

`predict` returns `OrbitError::UnknownShip` or `OrbitError::UnknownBody` for an
id that is not in the system. It returns an empty path when `samples` is 0 or
`duration` is not positive.

To preview a burn, call `predict_state` with the state after the burn and the
thrust to assume:

```rust
use yowie::orbit::{BodyId, DVec3, ShipId, System};

/// The path after an instant prograde change of `dv`, without making it.
fn preview_kick(
    sys: &System,
    ship: ShipId,
    dv: f64,
    horizon: f64,
    reference: BodyId,
) -> Vec<DVec3> {
    let Some(mut after) = sys.ship(ship).map(|s| s.state) else {
        return Vec::new();
    };
    after.vel += after.vel.normalize_or(DVec3::X) * dv;
    sys.predict_state(after, DVec3::ZERO, horizon, 90, Some(reference))
        .unwrap_or_default()
}
```

Prediction integrates the ship over the whole horizon, taking between 1 and 64
integration steps per sample, so it costs more than drawing a frame. Do not
predict on every frame. The space example (`examples/space/path.rs`) keeps the
last path and predicts again only when the ship has moved one sample along it,
or when the thrust, the time warp, the primary or the horizon changed. Between
predictions it moves the stored path by the primary's displacement.

## Time warp

To warp time, multiply each update's delta by a warp factor and pass the
result to `step`. Split a long span into several calls, so the integration
steps stay short. With the default 8 substeps, one `step(dt)` shortens its
integration steps to no less than `dt / 512`. A single `step(1000.0)`
therefore uses steps of about 2 time units or longer, which is too coarse for
a ship on the space example's 21-unit orbit around Vessa. The space example
caps each call at 0.25 time units:

```rust
use yowie::orbit::System;

/// The longest span one call to `step` covers.
const MAX_STEP: f64 = 0.25;

/// Advances the orbits by one update's `delta` seconds at `warp` times speed.
fn advance(sys: &mut System, delta: f32, warp: f32) {
    let span = f64::from(delta) * f64::from(warp);
    let steps = (span / MAX_STEP).ceil().max(1.0);
    for _ in 0..steps as u32 {
        sys.step(span / steps);
    }
}
```

Choose `MAX_STEP` for your units, as a small fraction of the period of the
tightest orbit a ship flies. Orbiting bodies are exact at any warp, and only
ships need short steps. A system with no ships can take a step of any length
in one call.

Warp the orbital simulation with your own factor, as above.
`Time::set_scale` speeds up the whole game, including the camera and every
animation, and is capped at 64. To pause the orbits, stop calling `step`.

## Drawing a system

Orbit positions are `f64` and the renderer takes `f32`. Neighbouring `f32`
values near 1.5e11 (1 AU in metres) are 16 384 apart, so a position converted
directly is off by up to 8 km. To keep the scene precise, choose a floating
origin near the camera (the ship, or the body the camera follows), subtract it
in `f64`, and convert the difference:

```rust
use yowie::Vec3;
use yowie::orbit::DVec3;

/// A system position relative to the floating origin, ready to draw.
fn to_scene(p: DVec3, origin: DVec3) -> Vec3 {
    (p - origin).as_vec3()
}
```

The space example uses the focused body or the ship as the origin, and Tab
moves the focus.

The orbit reference plane is XY with +Z north. The space example sets its
camera's `up` (and its sky's `up`) to `Vec3::Z`, so the orbit plane is
horizontal on screen:

```rust
use yowie::Vec3;
use yowie::gfx::scene::Camera3D;

/// A camera `dist` units from the origin, with +Z up to match the orbit plane.
fn camera(yaw: f32, pitch: f32, dist: f32) -> Camera3D {
    let (sp, cp) = pitch.sin_cos();
    let (sy, cy) = yaw.sin_cos();
    Camera3D {
        position: Vec3::new(dist * cp * cy, dist * cp * sy, dist * sp),
        up: Vec3::Z,
        ..Camera3D::perspective(Vec3::ZERO, Vec3::ZERO, 0.8)
            .with_depth_range((dist * 0.01).max(0.02), 20_000.0)
    }
}
```

To draw an orbit as a ring, step the true anomaly around a closed orbit and
take each position. The points are relative to the primary, so add the
primary's position before you subtract the origin. For an open orbit, keep the
true anomaly inside the asymptotes, `abs(nu) < acos(-1 / e)`.

```rust
use std::f64::consts::TAU;
use yowie::orbit::{DVec3, Elements};

/// `n + 1` points around a closed orbit, relative to its primary.
fn ring(mut el: Elements, mu: f64, n: usize) -> Vec<DVec3> {
    (0..=n)
        .map(|i| {
            el.true_anomaly = TAU * i as f64 / n as f64;
            el.state(mu).pos
        })
        .collect()
}
```

Draw the segments with `Scene::line`, which bodies in front hide, or with
`Scene::line_on_top`, which draws over everything; the space example draws its
predicted path on top. See [3D graphics](graphics-3d.md).

## A ship in a game

To fly a ship in a game, keep the `System` in your `Game` struct, set the
thrust and call `step` in `update`, and draw in `draw`. This game puts a ship
in a low orbit around a planet. W burns prograde, the comma and full stop keys
halve and double the time warp, and the predicted path is drawn around the
planet, which is the floating origin.

```rust,no_run
use yowie::gfx::scene::Camera3D;
use yowie::input::Key;
use yowie::orbit::{BodyId, DVec3, Elements, ShipId, State, System};
use yowie::{Color, Config, Context, Game, GameResult, Vec3};

/// The longest span one call to `step` covers.
const MAX_STEP: f64 = 0.25;

struct Flight {
    sys: System,
    planet: BodyId,
    ship: ShipId,
    warp: f32,
}

impl Game for Flight {
    fn init(_ctx: &mut Context) -> GameResult<Self> {
        let mut sys = System::new(1.0);
        let star = sys.add_fixed(175.0, State::default());
        let planet = sys.add_orbiting(1.2, star, Elements::circular(110.0))?;
        let at = sys.body_state(planet).unwrap_or_default();
        let orbit = Elements::circular(2.4).state(sys.g * 1.2);
        let ship = sys.add_ship(State {
            pos: at.pos + orbit.pos,
            vel: at.vel + orbit.vel,
        });
        Ok(Flight {
            sys,
            planet,
            ship,
            warp: 1.0,
        })
    }

    fn update(&mut self, ctx: &mut Context) -> GameResult {
        if ctx.input.key_pressed(Key::Escape) {
            ctx.quit();
        }
        if ctx.input.key_pressed(Key::Period) {
            self.warp = (self.warp * 2.0).min(256.0);
        }
        if ctx.input.key_pressed(Key::Comma) {
            self.warp = (self.warp / 2.0).max(1.0);
        }
        let prograde = self
            .sys
            .ship(self.ship)
            .map_or(DVec3::X, |s| s.state.vel.normalize_or(DVec3::X));
        let accel = if ctx.input.key_down(Key::W) {
            prograde * 0.01
        } else {
            DVec3::ZERO
        };
        self.sys.set_thrust(self.ship, accel)?;

        let span = f64::from(ctx.time.delta()) * f64::from(self.warp);
        let steps = (span / MAX_STEP).ceil().max(1.0);
        for _ in 0..steps as u32 {
            self.sys.step(span / steps);
        }
        Ok(())
    }

    fn draw(&mut self, ctx: &mut Context) -> GameResult {
        let origin = self
            .sys
            .body_state(self.planet)
            .map_or(DVec3::ZERO, |s| s.pos);
        // A real game keeps this path between frames; see "Predicting a path".
        let path = self.sys.predict(self.ship, 30.0, 60, Some(self.planet))?;
        let cam = Camera3D {
            up: Vec3::Z,
            ..Camera3D::perspective(Vec3::new(0.0, -8.0, 6.0), Vec3::ZERO, 0.8)
        };
        let color = Color::rgb8(120, 220, 255);
        ctx.gfx.scene(&cam, |s| {
            for pair in path.windows(2) {
                let a = (pair[0] - origin).as_vec3();
                let b = (pair[1] - origin).as_vec3();
                s.line(a, b, color);
            }
        });
        Ok(())
    }
}

fn main() -> Result<(), yowie::Error> {
    yowie::run::<Flight>(Config::new("Flight"))
}
```

## Our solar system

`yowie::orbit::sol` holds real data in metres, kilograms and seconds.

| Item | Contents |
|---|---|
| `AU`, `DAY`, `YEAR`, `C` | The astronomical unit, a day of 86 400 s, a Julian year of 365.25 days, the speed of light |
| `MU_SUN`, `MU_EARTH`, `MU_MOON`, `MU_JUPITER` | Standard gravitational parameters in m³/s² |
| `MOON_DISTANCE` | The mean Earth-Moon distance |
| `SUN`, `MERCURY` to `NEPTUNE`, `PLUTO` | The Sun, the eight planets and Pluto |
| `MOON`, `PHOBOS`, `IO`, `TITAN`, `CHARON` and others | 21 major moons |
| `ALL` | All 31 bodies, each primary before its moons |
| `by_name(name)` | A body looked up by name, ignoring case |
| `load(system)` | Adds every body to a `System` |

Each `sol::Body` has a `name`, a `mass` in kilograms, a mean `radius` in metres
and an `orbit`, which is `None` for the Sun. `Body::mu()` returns `G * mass`.
A `sol::Orbit` names its primary and gives its elements with a mean anomaly at
the J2000 epoch; `Orbit::elements()` converts it to `Elements`. `sol::Body` is
a different type from `yowie::orbit::Body`, the point mass that `Simulation`
integrates.

The planets use JPL mean elements at J2000, referred to the ecliptic, which is
the XY plane. Earth's orbit is that of the Earth-Moon barycentre. Each orbit is
a fixed ellipse, with no precession and no perturbations. The Moon has a mean
orbit in the ecliptic with its angles at J2000. The other moons give only the
semi-major axis, the eccentricity and the inclination to their planet's
equator. Their other angles are 0 and their orbits are not rotated into the
ecliptic, so set `node` and `true_anomaly` yourself to spread them out.

To load the whole system, create a `System` with the SI constant and call
`sol::load`. It fixes the Sun at the origin, places every body as at J2000 at
the system's time 0, and returns each body with its `BodyId`.

```rust
use yowie::orbit::{System, sol};

fn main() {
    let mut sys = System::default();
    let bodies = sol::load(&mut sys);
    let earth = bodies
        .iter()
        .find(|(b, _)| b.name == "Earth")
        .map(|&(_, id)| id);

    sys.step(30.0 * sol::DAY);
    if let Some(state) = earth.and_then(|id| sys.body_state(id)) {
        println!("Earth is {:.4} AU from the Sun", state.pos.length() / sol::AU);
    }
}
```

## N-body gravity

`Simulation` integrates point masses under their mutual gravity. Use it when
every body must pull on every other: binary stars, a cluster, or planets that
perturb each other. For planets on fixed orbits with ships among them,
`System` is cheaper and does not drift.

Each `yowie::orbit::Body` has a `pos`, a `vel` and a `mass`. A body with zero
mass feels gravity and exerts none. `Simulation::new(bodies)` uses `G` and no
softening. Its fields `bodies`, `g`, `softening` and `time` are public, and you
may change them between steps.

To advance, call `step(dt)`. It uses the leapfrog (kick-drift-kick) method,
which keeps the energy error bounded over long runs and conserves momentum and
angular momentum to rounding error. Choose `dt` at about 1/200 of the shortest
orbit period or less. Each step visits every pair of bodies, so its cost grows
with the square of the number of bodies. The accelerations from the end of one
step are reused at the start of the next. Changing the bodies, `g` or
`softening` between steps costs one extra evaluation.

```rust
use yowie::orbit::{Body, Elements, Simulation};

/// Two stars of masses `m1` and `m2`, `d` apart on a circular orbit about
/// their centre of mass, with g = 1.
fn binary(m1: f64, m2: f64, d: f64) -> Simulation {
    let total = m1 + m2;
    let rel = Elements::circular(d).state(total);
    let mut sim = Simulation::new(vec![
        Body {
            pos: -rel.pos * (m2 / total),
            vel: -rel.vel * (m2 / total),
            mass: m1,
        },
        Body {
            pos: rel.pos * (m1 / total),
            vel: rel.vel * (m1 / total),
            mass: m2,
        },
    ]);
    sim.g = 1.0;
    sim
}

fn main() {
    let mut sim = binary(300.0, 100.0, 20.0);
    let start = sim.energy();
    // One orbit takes about 28 time units, so 0.1 is about 1/280 of it.
    for _ in 0..1000 {
        sim.step(0.1);
    }
    println!("energy changed by {:.2e}", (sim.energy() - start) / start);
    assert!(sim.momentum().length() < 1e-9);
}
```

| Method | Returns |
|---|---|
| `energy()` | Kinetic plus potential energy, ignoring softening |
| `momentum()` | Total linear momentum |
| `angular_momentum()` | Total angular momentum about the origin |
| `barycenter()` | The centre of mass, or the origin when all masses are zero |
| `accelerations(&mut out)` | Every body's acceleration, written into `out` |
| `field_at(p)` | The acceleration a test mass feels at `p` |

To fly a probe or a ship through a `Simulation`, call `rk4(pos, vel, dt,
accel)` with a closure that returns the acceleration at a position. Use
`Simulation::field_at` for the gravity and add the engine's thrust. The bodies
stay where they are for the whole call, so keep `dt` short. The free function
`field_at(bodies, g, softening, p)` computes the same field for any slice of
bodies.

```rust
use yowie::orbit::{DVec3, Simulation, rk4};

/// Moves a probe through the simulation's gravity with its engine burning.
fn fly_probe(
    sim: &Simulation,
    pos: DVec3,
    vel: DVec3,
    thrust: DVec3,
    dt: f64,
) -> (DVec3, DVec3) {
    rk4(pos, vel, dt, |p, _vel| sim.field_at(p) + thrust)
}
```

`softening` is a distance added in quadrature to every separation, so the
pull of a body at distance `d` is `g m d / (d² + softening²)^(3/2)`. Use it to
keep close encounters finite. Because `energy()` ignores softening, the energy
is conserved only when `softening` is 0.

## Saving a game

`State`, `Elements`, `Ship`, `BodyId`, `ShipId` and `yowie::orbit::Body`
implement serde's `Serialize` and `Deserialize`. `System` and `Simulation` do
not, and a `System`'s time can only move forward through `step`.

To save a `System`, store its time and its ships. To load, build the bodies
again exactly as at the start, call `step` with the saved time before adding
any ship (with no ships the step is exact), then add the ships back in the
same order so their `ShipId`s match. If the game moved a fixed body with
`set_fixed_state`, save that body's state too and set it again after the step.

```rust
use serde::{Deserialize, Serialize};
use yowie::orbit::{Ship, ShipId, System};

#[derive(Serialize, Deserialize)]
struct OrbitSave {
    time: f64,
    ships: Vec<Ship>,
}

fn save(sys: &System, ships: &[ShipId]) -> OrbitSave {
    OrbitSave {
        time: sys.time(),
        ships: ships.iter().filter_map(|&id| sys.ship(id).copied()).collect(),
    }
}

/// Brings `fresh`, the bodies as first built and no ships, to the saved moment.
fn load(mut fresh: System, saved: &OrbitSave) -> (System, Vec<ShipId>) {
    fresh.step(saved.time);
    let ships = saved
        .ships
        .iter()
        .map(|s| {
            let id = fresh.add_ship(s.state);
            let _ = fresh.set_thrust(id, s.thrust);
            id
        })
        .collect();
    (fresh, ships)
}
```

The derive needs `serde` with its `derive` feature in your `Cargo.toml`. To
write the save to disk, pass it to `yowie::save::Store::write`; see
[services](services.md). To save a `Simulation`, store its `bodies` and `time`
and set them on a new one.

## The examples

```bash
cargo run --example space
cargo run --example space -- --focus Grond --dist 40 --warp 20
```

The space example flies a ship through a fictional system with g = 1: the star
Kaal, seven planets with ten moons, a comet on an eccentric, inclined orbit,
and 300 belt rocks placed from their elements. The ship starts in a low orbit
around Vessa. Its predicted path is drawn in the frame of the body it
orbits, and the Ship window shows that orbit's semi-major axis, eccentricity,
apsides and period. The code is in `examples/space/`: `system.rs` defines the
system, `path.rs` caches the prediction, and `view.rs` draws it.

| Input | Does |
|---|---|
| W, S | Thrust prograde, retrograde |
| A, D | Thrust sideways |
| Q, E | Thrust out of the orbital plane, one side or the other |
| Tab | Moves the camera focus, and the floating origin, to the next body |
| Drag, wheel | Orbit the camera, zoom |
| Thrust and Time warp sliders | Thrust acceleration from 0 to 0.05, warp from 1 to 200 |
| Escape | Quit |

| Flag | Does |
|---|---|
| `--focus <name>` | Starts focused on `Ship`, `Kaal`, a planet such as `Grond`, or `"Comet Iri"` |
| `--dist <n>` | Starting camera distance (default 14) |
| `--warp <n>` | Starting time warp (default 1) |
| `--autoplay` | Burns prograde from 0.5 s to 1.5 s of game time, so a headless run shows a changed path |

```bash
cargo run --example solar -- --select Halcyon
```

The solar example draws a stylised system on the entity component system. The
sun, planets and moons come from a scene file and a moon prefab, the transform
hierarchy carries each moon around its planet, and 400 asteroids draw in one
instanced call. Its orbits are circular `Orbit` components that a system
advances; it does not use `yowie::orbit`. Read it for scenes, prefabs and the
hierarchy, which [entities and systems](ecs.md) covers. `--select <name>`
starts with a body selected (`Sun`, `Ember`, `Verdis`, `Halcyon`, `Umber`,
`Gale`, or a moon such as `"Gale II"`), `--timings` shows each system's run
time in headless runs, and `--panels` opens the console panels.

Both examples also take the engine's `--shot <file.png>` and `--seconds <n>`
flags.
