//! Panels for the simulation crates a game may use: ECS worlds, physics
//! worlds and the audio mixer. Each exists only with its cargo feature.
//!
//! The console cannot hold a game's world, so the game hands it a
//! snapshot each frame (`feed_world`, `feed_physics_2d`) or a shared
//! handle once (`attach_mixer`).

#[cfg(feature = "ecs")]
use yowie_core::Vec2;
#[cfg(feature = "ecs")]
use yowie_ecs::{Name, Schedule, SystemStat, World};
#[cfg(feature = "audio")]
use yowie_audio::Mixer;
#[cfg(feature = "phys")]
use yowie_phys::{World2D, World3D};

use crate::console::{Console, Core};
use yowie_ui::Frame;

/// The most entities one world snapshot lists.
#[cfg(feature = "ecs")]
pub const MAX_ENTITY_ROWS: usize = 500;

/// One entity as the Entities panel shows it.
#[cfg(feature = "ecs")]
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct EntityRow {
    pub(crate) id: u32,
    pub(crate) name: String,
    pub(crate) components: usize,
    pub(crate) position: Option<Vec2>,
}

/// A world as the Entities panel shows it.
#[cfg(feature = "ecs")]
#[derive(Clone, Debug, Default)]
pub(crate) struct WorldInfo {
    pub(crate) name: String,
    pub(crate) entities: usize,
    pub(crate) rows: Vec<EntityRow>,
    pub(crate) systems: Vec<SystemStat>,
    pub(crate) updates: u64,
    pub(crate) registered: Vec<String>,
}

/// A physics world's counts.
#[cfg(feature = "phys")]
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct PhysInfo {
    pub(crate) name: String,
    pub(crate) dimensions: u8,
    pub(crate) bodies: usize,
    pub(crate) colliders: usize,
    pub(crate) gravity: String,
}

/// What the simulation panels hold.
#[derive(Default)]
pub(crate) struct Sim {
    #[cfg(feature = "ecs")]
    pub(crate) worlds: Vec<WorldInfo>,
    #[cfg(feature = "ecs")]
    pub(crate) toggles: Vec<(String, bool)>,
    #[cfg(feature = "ecs")]
    pub(crate) selected: Option<u32>,
    #[cfg(feature = "ecs")]
    pub(crate) filter: String,
    #[cfg(feature = "phys")]
    pub(crate) physics: Vec<PhysInfo>,
    #[cfg(feature = "audio")]
    pub(crate) mixer: Option<Mixer>,
}

impl Sim {
    /// The names of the simulation panels that have something to show.
    pub(crate) fn panel_names(&self) -> Vec<&'static str> {
        #[allow(unused_mut)]
        let mut names = Vec::new();
        #[cfg(feature = "ecs")]
        if !self.worlds.is_empty() {
            names.push("Entities");
        }
        #[cfg(feature = "phys")]
        if !self.physics.is_empty() {
            names.push("Physics");
        }
        #[cfg(feature = "audio")]
        if self.mixer.is_some() {
            names.push("Audio");
        }
        names
    }
}

impl Console {
    /// Shows an ECS world in the Entities panel: its entities (the first
    /// 500), its systems with their timings, and a switch for each
    /// system. Call it every frame the panel should stay current; it is
    /// cheap while the panels are hidden. Switches the player flipped are
    /// applied to `schedule` here.
    #[cfg(feature = "ecs")]
    pub fn feed_world(&mut self, name: &str, world: &World, schedule: Option<&mut Schedule>) {
        let sim = &mut self.core.sim;
        let (mut systems, mut updates) = (Vec::new(), 0);
        if let Some(s) = schedule {
            for (n, on) in sim.toggles.drain(..) {
                s.set_enabled(&n, on);
            }
            systems = s.stats();
            updates = s.updates();
        }
        let shown = self.core.panels_open && self.enabled;
        let mut rows = Vec::new();
        if shown {
            for e in world.iter().take(MAX_ENTITY_ROWS) {
                let name = e.get::<&Name>().map(|n| n.0.clone()).unwrap_or_default();
                let position = e.get::<&yowie_core::Transform2D>().map(|t| t.position);
                rows.push(EntityRow {
                    id: e.entity().id(),
                    name,
                    components: e.component_types().count(),
                    position,
                });
            }
        }
        let info = WorldInfo {
            name: name.to_owned(),
            entities: world.len() as usize,
            rows,
            systems,
            updates,
            registered: world.registered_components().iter().map(|s| (*s).to_owned()).collect(),
        };
        let sim = &mut self.core.sim;
        match sim.worlds.iter_mut().find(|w| w.name == name) {
            Some(w) => *w = info,
            None => sim.worlds.push(info),
        }
    }

    /// Shows a 2D physics world's body and collider counts in the Physics
    /// panel.
    #[cfg(feature = "phys")]
    pub fn feed_physics_2d(&mut self, name: &str, world: &World2D) {
        let g = world.gravity();
        self.feed_physics(PhysInfo {
            name: name.to_owned(),
            dimensions: 2,
            bodies: world.body_count(),
            colliders: world.collider_count(),
            gravity: format!("({:.2}, {:.2})", g.x, g.y),
        });
    }

    /// Shows a 3D physics world's body and collider counts in the Physics
    /// panel.
    #[cfg(feature = "phys")]
    pub fn feed_physics_3d(&mut self, name: &str, world: &World3D) {
        let g = world.gravity();
        self.feed_physics(PhysInfo {
            name: name.to_owned(),
            dimensions: 3,
            bodies: world.body_count(),
            colliders: world.collider_count(),
            gravity: format!("({:.2}, {:.2}, {:.2})", g.x, g.y, g.z),
        });
    }

    #[cfg(feature = "phys")]
    fn feed_physics(&mut self, info: PhysInfo) {
        let list = &mut self.core.sim.physics;
        match list.iter_mut().find(|p| p.name == info.name) {
            Some(p) => *p = info,
            None => list.push(info),
        }
    }

    /// Shows the mixer in the Audio panel, with sliders and switches for
    /// its buses. The handle is shared with the game's own.
    #[cfg(feature = "audio")]
    pub fn attach_mixer(&mut self, mixer: &Mixer) {
        self.core.sim.mixer = Some(mixer.clone());
    }
}

impl Core {
    /// Draws the Entities, Physics or Audio panel named `name`. Returns
    /// whether `name` was one of them.
    #[allow(unused_variables)]
    pub(crate) fn sim_panel(&mut self, name: &str, f: &mut Frame<'_>) -> bool {
        match name {
            #[cfg(feature = "ecs")]
            "Entities" => {
                self.entities_panel(f);
                true
            }
            #[cfg(feature = "phys")]
            "Physics" => {
                self.physics_panel(f);
                true
            }
            #[cfg(feature = "audio")]
            "Audio" => {
                self.audio_panel(f);
                true
            }
            _ => false,
        }
    }

    #[cfg(feature = "ecs")]
    fn entities_panel(&mut self, f: &mut Frame<'_>) {
        use yowie_ui::{Column, Size};
        let sim = &mut self.sim;
        let Some(world) = sim.worlds.first().cloned() else {
            return;
        };
        for w in &sim.worlds {
            f.label(&format!("{}: {} entities, {} systems", w.name, w.entities, w.systems.len()));
        }
        f.label_dim(&format!("{} registered component types", world.registered.len()));
        f.separator();
        f.heading("Systems");
        if world.systems.is_empty() {
            f.label_dim("no schedule attached");
        }
        for s in &world.systems {
            let mut on = s.enabled;
            f.with_id(&s.name, |f| {
                f.row(&[Size::Fixed(150.0), Size::Fill(1.0)], |f| {
                    if f.checkbox(&s.name, &mut on).changed {
                        sim.toggles.push((s.name.clone(), on));
                    }
                    f.label_dim(&format!(
                        "{:.3} ms  max {:.3}  runs {}",
                        s.last.as_secs_f64() * 1000.0,
                        s.max.as_secs_f64() * 1000.0,
                        s.runs
                    ));
                });
            });
        }
        f.separator();
        f.heading("Entities");
        f.text_field_with(
            "filter",
            &mut sim.filter,
            &yowie_ui::TextOptions {
                placeholder: "filter by name or id",
                ..Default::default()
            },
        );
        let needle = sim.filter.to_ascii_lowercase();
        let rows: Vec<_> = world
            .rows
            .iter()
            .filter(|r| {
                needle.is_empty()
                    || r.name.to_ascii_lowercase().contains(&needle)
                    || r.id.to_string().contains(&needle)
            })
            .collect();
        let mut selected = sim.selected.and_then(|id| rows.iter().position(|r| r.id == id));
        let cols = [Column::fixed("id", 60.0), Column::fill("name"), Column::fixed("parts", 60.0), Column::fixed("position", 130.0)];
        let t = f.table("entities", &cols, rows.len(), 180.0, &mut selected, |f, row, col| {
            let r = rows[row];
            match col {
                0 => f.label(&r.id.to_string()),
                1 => f.label(&r.name),
                2 => f.label(&r.components.to_string()),
                _ => f.label(&r.position.map_or(String::new(), |p| format!("{:.1}, {:.1}", p.x, p.y))),
            };
        });
        if let Some(i) = t.row_clicked.or(selected) {
            sim.selected = rows.get(i).map(|r| r.id);
        }
        if world.rows.len() >= MAX_ENTITY_ROWS {
            f.label_dim(&format!("showing the first {MAX_ENTITY_ROWS} of {}", world.entities));
        }
    }

    #[cfg(feature = "phys")]
    fn physics_panel(&mut self, f: &mut Frame<'_>) {
        for p in &self.sim.physics {
            f.heading(&format!("{} ({}D)", p.name, p.dimensions));
            f.label(&format!("{} bodies, {} colliders", p.bodies, p.colliders));
            f.label_dim(&format!("gravity {}", p.gravity));
            f.separator();
        }
    }

    #[cfg(feature = "audio")]
    fn audio_panel(&mut self, f: &mut Frame<'_>) {
        let Some(mixer) = self.sim.mixer.clone() else {
            return;
        };
        f.label(&format!("sample rate {} Hz", mixer.rate()));
        f.label_dim("the mixer does not report its own load");
        f.separator();
        for bus in [mixer.master(), mixer.music(), mixer.effects()] {
            f.with_id(bus.name(), |f| {
                f.heading(bus.name());
                let mut v = bus.volume();
                if f.slider("volume", &mut v, 0.0..=1.0).changed {
                    bus.set_volume(v);
                }
                let (mut mute, mut solo, mut paused) = (bus.muted(), bus.soloed(), bus.paused());
                f.columns(3, |f| {
                    if f.checkbox("mute", &mut mute).changed {
                        bus.set_mute(mute);
                    }
                    if f.checkbox("solo", &mut solo).changed {
                        bus.set_solo(solo);
                    }
                    if f.checkbox("pause", &mut paused).changed {
                        bus.set_paused(paused);
                    }
                });
            });
        }
    }
}
