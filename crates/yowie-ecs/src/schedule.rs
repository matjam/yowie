//! Systems and the schedule that runs them in ordered stages.

use std::time::{Duration, Instant};

use crate::world::World;

/// Run phase of a system. Stages run in the order listed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Stage {
    /// Runs once, during the first [`Schedule::run`].
    Startup,
    /// Runs every update before [`Stage::Update`]; typically input handling.
    PreUpdate,
    /// Runs every update; typically gameplay.
    Update,
    /// Runs every update after [`Stage::Update`]; typically transform
    /// propagation and cleanup.
    PostUpdate,
}

const STAGES: [Stage; 4] = [
    Stage::Startup,
    Stage::PreUpdate,
    Stage::Update,
    Stage::PostUpdate,
];

type Condition = Box<dyn FnMut(&World) -> bool>;

/// A named unit of game logic over the world, with optional run conditions.
///
/// Any `FnMut(&mut World)` is a system function, so plain `fn`s and closures
/// both work.
pub struct System {
    name: String,
    run: Box<dyn FnMut(&mut World)>,
    conditions: Vec<Condition>,
}

impl System {
    /// Creates a system. The name identifies it in [`Schedule::stats`] and
    /// [`Schedule::set_enabled`]; names should be unique.
    pub fn new(name: &str, run: impl FnMut(&mut World) + 'static) -> Self {
        Self {
            name: name.to_owned(),
            run: Box::new(run),
            conditions: Vec::new(),
        }
    }

    /// Adds a run condition. The system runs only when every condition
    /// returns true; conditions are checked in the order added, each time the
    /// schedule reaches the system.
    pub fn run_if(mut self, condition: impl FnMut(&World) -> bool + 'static) -> Self {
        self.conditions.push(Box::new(condition));
        self
    }
}

/// Timing and run counts for one system, for the debug console.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SystemStat {
    /// The system's name.
    pub name: String,
    /// The stage it runs in.
    pub stage: Stage,
    /// Whether the system is enabled.
    pub enabled: bool,
    /// Duration of the most recent run.
    pub last: Duration,
    /// Longest run so far.
    pub max: Duration,
    /// Total time spent in all runs.
    pub total: Duration,
    /// Number of runs.
    pub runs: u64,
    /// Number of times it was passed over because it was disabled or a run
    /// condition was false.
    pub skipped: u64,
}

struct Slot {
    system: System,
    stat: SystemStat,
}

/// Runs systems in stage order, applying deferred commands after each one.
///
/// Within a stage, systems run in the order they were added. After the
/// last stage the schedule rotates the world's event queues, so an event is
/// readable in the update that sent it and the next.
#[derive(Default)]
pub struct Schedule {
    slots: [Vec<Slot>; 4],
    started: bool,
    updates: u64,
}

impl Schedule {
    /// Creates an empty schedule.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a system to a stage.
    pub fn add(&mut self, stage: Stage, system: System) -> &mut Self {
        let stat = SystemStat {
            name: system.name.clone(),
            stage,
            enabled: true,
            last: Duration::ZERO,
            max: Duration::ZERO,
            total: Duration::ZERO,
            runs: 0,
            skipped: 0,
        };
        self.slots[stage as usize].push(Slot { system, stat });
        self
    }

    /// Adds a function or closure as a system with no conditions.
    pub fn add_system(
        &mut self,
        stage: Stage,
        name: &str,
        run: impl FnMut(&mut World) + 'static,
    ) -> &mut Self {
        self.add(stage, System::new(name, run))
    }

    /// Enables or disables every system called `name`. Returns false when no
    /// system has that name.
    pub fn set_enabled(&mut self, name: &str, enabled: bool) -> bool {
        let mut found = false;
        for slot in self.slots.iter_mut().flatten() {
            if slot.system.name == name {
                slot.stat.enabled = enabled;
                found = true;
            }
        }
        found
    }

    /// Returns the stats of every system in run order.
    pub fn stats(&self) -> Vec<SystemStat> {
        self.slots
            .iter()
            .flatten()
            .map(|s| s.stat.clone())
            .collect()
    }

    /// Number of completed calls to [`Schedule::run`].
    pub fn updates(&self) -> u64 {
        self.updates
    }

    /// Runs one update: the startup stage first if it has not run yet, then
    /// pre-update, update and post-update, then rotates the event queues.
    pub fn run(&mut self, world: &mut World) {
        if !self.started {
            self.started = true;
            self.run_stage(Stage::Startup, world);
        }
        for stage in &STAGES[1..] {
            self.run_stage(*stage, world);
        }
        world.update_events();
        self.updates += 1;
    }

    fn run_stage(&mut self, stage: Stage, world: &mut World) {
        for slot in &mut self.slots[stage as usize] {
            let runnable = slot.stat.enabled && slot.system.conditions.iter_mut().all(|c| c(world));
            if !runnable {
                slot.stat.skipped += 1;
                continue;
            }
            let start = Instant::now();
            (slot.system.run)(world);
            let took = start.elapsed();
            slot.stat.last = took;
            slot.stat.max = slot.stat.max.max(took);
            slot.stat.total += took;
            slot.stat.runs += 1;
            world.flush_commands();
        }
    }
}

/// Run condition: true when a resource of type `T` exists.
pub fn resource_exists<T: 'static>() -> impl FnMut(&World) -> bool {
    |world| world.has_resource::<T>()
}

/// Run condition: true when at least one event of type `T` is readable.
pub fn has_events<T: 'static>() -> impl FnMut(&World) -> bool {
    |world| world.read_events::<T>().next().is_some()
}

/// Run condition: true when `condition` is false.
pub fn not(mut condition: impl FnMut(&World) -> bool) -> impl FnMut(&World) -> bool {
    move |world| !condition(world)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;

    fn log_system(log: &Rc<RefCell<Vec<&'static str>>>, tag: &'static str) -> System {
        let log = Rc::clone(log);
        System::new(tag, move |_| log.borrow_mut().push(tag))
    }

    #[test]
    fn stages_run_in_order_and_startup_once() {
        let log = Rc::new(RefCell::new(Vec::new()));
        let mut s = Schedule::new();
        // Added out of order on purpose.
        s.add(Stage::PostUpdate, log_system(&log, "post"));
        s.add(Stage::Update, log_system(&log, "u1"));
        s.add(Stage::Startup, log_system(&log, "start"));
        s.add(Stage::Update, log_system(&log, "u2"));
        s.add(Stage::PreUpdate, log_system(&log, "pre"));
        let mut w = World::new();
        s.run(&mut w);
        s.run(&mut w);
        assert_eq!(
            *log.borrow(),
            [
                "start", "pre", "u1", "u2", "post", "pre", "u1", "u2", "post"
            ]
        );
        assert_eq!(s.updates(), 2);
    }

    #[test]
    fn conditions_and_enable_flags() {
        struct Go;
        struct Ping;
        let log = Rc::new(RefCell::new(Vec::new()));
        let mut s = Schedule::new();
        s.add(
            Stage::Update,
            log_system(&log, "needs-go").run_if(resource_exists::<Go>()),
        );
        s.add(
            Stage::Update,
            log_system(&log, "on-ping").run_if(has_events::<Ping>()),
        );
        s.add(
            Stage::Update,
            log_system(&log, "no-go").run_if(not(resource_exists::<Go>())),
        );
        let mut w = World::new();
        s.run(&mut w);
        assert_eq!(*log.borrow(), ["no-go"]);

        log.borrow_mut().clear();
        w.insert_resource(Go);
        w.send_event(Ping);
        s.run(&mut w);
        assert_eq!(*log.borrow(), ["needs-go", "on-ping"]);

        // The ping was sent before the last rotation, so it is still readable.
        log.borrow_mut().clear();
        assert!(s.set_enabled("needs-go", false));
        assert!(!s.set_enabled("missing", false));
        s.run(&mut w);
        assert_eq!(*log.borrow(), ["on-ping"]);

        // Now it has been dropped.
        log.borrow_mut().clear();
        s.run(&mut w);
        assert!(log.borrow().is_empty());
    }

    #[test]
    fn stats_record_runs_skips_and_time() {
        let mut s = Schedule::new();
        s.add_system(Stage::Update, "sleepy", |_| {
            std::thread::sleep(Duration::from_millis(2));
        });
        s.add(
            Stage::Update,
            System::new("never", |_| {}).run_if(|_| false),
        );
        let mut w = World::new();
        s.run(&mut w);
        s.run(&mut w);
        let stats = s.stats();
        assert_eq!((stats[0].runs, stats[0].skipped), (2, 0));
        assert!(stats[0].last >= Duration::from_millis(2));
        assert!(stats[0].total >= stats[0].max);
        assert_eq!((stats[1].runs, stats[1].skipped), (0, 2));
        assert_eq!(stats[1].stage, Stage::Update);
    }

    #[test]
    fn commands_apply_between_systems() {
        let mut s = Schedule::new();
        s.add_system(Stage::Update, "spawn", |w| {
            let mut cmds = w.commands();
            cmds.run(|w| {
                w.insert_resource(1_u8);
            });
        });
        s.add(
            Stage::Update,
            System::new("see", |w| {
                assert!(w.has_resource::<u8>());
            }),
        );
        let mut w = World::new();
        s.run(&mut w);
        assert!(w.has_resource::<u8>());
    }
}
