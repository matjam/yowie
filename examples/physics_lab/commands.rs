//! The console commands and variables that drive the lab.
//!
//! Variables (`set <name> <value>`): `lab.paused`, `lab.debug`,
//! `lab.contacts`, `lab.gravity`, `wheel.speed`, `wheel.torque`.
//! Commands: `spawn`, `reset`, `ragdoll`, `kick`, `pause`, `debug`.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

use yowie::console::{BoolVar, Console, FloatVar};

use crate::lab::Kind;

/// Something the console asked the lab to do on the next update.
pub enum Request {
    Spawn(usize, Option<Kind>),
    Reset,
    Ragdoll,
    Kick(f32),
}

/// The variables the lab reads each frame.
pub struct Vars {
    pub paused: BoolVar,
    pub debug: BoolVar,
    pub contacts: BoolVar,
    /// Gravity's strength in units per second squared, pointing down.
    pub gravity: FloatVar,
    pub wheel_speed: FloatVar,
    pub wheel_torque: FloatVar,
}

/// Shared with the command handlers, which run inside the console.
pub type Queue = Rc<RefCell<VecDeque<Request>>>;

/// Registers the lab's variables and commands.
pub fn register(console: &mut Console) -> (Vars, Queue) {
    let vars = Vars {
        paused: console.boolean("lab.paused", false, "stops the simulation"),
        debug: console.boolean("lab.debug", true, "draws colliders and joints as lines"),
        contacts: console.boolean("lab.contacts", true, "draws contact points and normals"),
        gravity: console.float("lab.gravity", 9.8, "gravity in units per second squared"),
        wheel_speed: console.float(
            "wheel.speed",
            1.5,
            "the paddle wheel's motor speed in rad/s",
        ),
        wheel_torque: console.float("wheel.torque", 400.0, "the paddle wheel's motor torque"),
    };
    let queue: Queue = Rc::default();

    let q = queue.clone();
    console.register_completing(
        "spawn",
        "spawn [count] [capsule|hull|wedge|box|sphere]: throw more bodies",
        move |args| {
            let mut count = 10;
            let mut kind = None;
            for a in args {
                if let Ok(n) = a.parse::<usize>() {
                    count = n;
                } else if let Some(k) = Kind::ALL.iter().find(|k| k.name() == a) {
                    kind = Some(*k);
                } else {
                    return Err(format!("spawn: unknown argument {a:?}"));
                }
            }
            q.borrow_mut().push_back(Request::Spawn(count, kind));
            Ok(format!("throwing {count}"))
        },
        |args| {
            let prefix = args.last().map_or("", String::as_str);
            Kind::ALL
                .iter()
                .map(|k| k.name())
                .filter(|n| n.starts_with(prefix))
                .map(str::to_owned)
                .collect()
        },
    );

    let q = queue.clone();
    console.register(
        "reset",
        "reset: throw the debris and the ragdoll in again",
        move |_| {
            q.borrow_mut().push_back(Request::Reset);
            Ok("dropped".into())
        },
    );
    let q = queue.clone();
    console.register("ragdoll", "ragdoll: drop another ragdoll", move |_| {
        q.borrow_mut().push_back(Request::Ragdoll);
        Ok("ragdoll dropped".into())
    });
    let q = queue.clone();
    console.register(
        "kick",
        "kick [speed]: throw every loose body upward",
        move |args| {
            let speed = args.first().map_or(Ok(8.0), |a| a.parse::<f32>());
            let speed = speed.map_err(|_| "kick: speed must be a number".to_string())?;
            q.borrow_mut().push_back(Request::Kick(speed));
            Ok(format!("kicked at {speed}"))
        },
    );

    let paused = vars.paused.clone();
    console.register("pause", "pause: stop or resume the simulation", move |_| {
        paused.set(!paused.get());
        Ok(if paused.get() { "paused" } else { "running" }.into())
    });
    let debug = vars.debug.clone();
    console.register("debug", "debug: show or hide the debug lines", move |_| {
        debug.set(!debug.get());
        Ok(if debug.get() {
            "debug lines on"
        } else {
            "debug lines off"
        }
        .into())
    });
    let gravity = vars.gravity.clone();
    console.register(
        "gravity",
        "gravity [g]: show or set gravity (9.8 is Earth, 0 floats)",
        move |args| match args.first() {
            None => Ok(format!("gravity {}", gravity.get())),
            Some(a) => {
                let g = a
                    .parse::<f32>()
                    .map_err(|_| "gravity: needs a number".to_string())?;
                gravity.set(g);
                Ok(format!("gravity {g}"))
            }
        },
    );
    (vars, queue)
}
