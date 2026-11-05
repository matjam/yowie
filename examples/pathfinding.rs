//! The grid crate: paint walls with the left mouse button (right erases),
//! and watch A* route from the start to the cell under the pointer, a
//! Dijkstra map shade every reachable cell by distance, field of view light
//! what the start can see, and flood fill count the connected region. The
//! middle button moves the start, D toggles diagonal moves, R rolls a new
//! random map, F5 saves the map and F9 loads it through `yowie-save`.
//!
//! In a headless run the goal is fixed, so the picture repeats.

#[path = "common/mod.rs"]
mod common;

use serde::{Deserialize, Serialize};
use yowie::gfx::Font;
use yowie::grid::{BLOCKED, Diagonal, Grid, Neighbourhood, Pathfinder, dijkstra, flood_fill, fov, line};
use yowie::input::{Key, MouseButton};
use yowie::rng::Rng;
use yowie::save::Store;
use yowie::{Color, Config, Context, Game, GameResult, IVec2, Rect, ivec2, vec2};

const COLS: usize = 40;
const ROWS: usize = 24;
const CELL: f32 = 24.0;

#[derive(Serialize, Deserialize)]
struct MapFile {
    walls: Vec<bool>,
    start: (i32, i32),
}

struct Paths {
    font: Font,
    walls: Grid<bool>,
    start: IVec2,
    diagonal: bool,
    random: Rng,
    store: Store,
    finder: Pathfinder,
    status: String,
}

impl Paths {
    /// Scatters walls and a few longer barriers.
    fn roll(&mut self) {
        self.walls.fill(false);
        for y in 0..ROWS as i32 {
            for x in 0..COLS as i32 {
                let wall = self.random.chance(0.18);
                self.walls.set(ivec2(x, y), wall);
            }
        }
        for _ in 0..6 {
            let x = self.random.range(0, COLS as i32);
            let y = self.random.range(0, ROWS as i32);
            for i in 0..8 {
                let p = if self.random.chance(0.5) {
                    ivec2(x + i, y)
                } else {
                    ivec2(x, y + i)
                };
                self.walls.set(p, true);
            }
        }
        self.walls.set(self.start, false);
    }

    fn rule(&self) -> Diagonal {
        if self.diagonal {
            Diagonal::IfBothClear
        } else {
            Diagonal::Never
        }
    }

    fn cell_at(point: yowie::Vec2) -> IVec2 {
        ivec2((point.x / CELL).floor() as i32, (point.y / CELL).floor() as i32)
    }
}

/// The cost of stepping onto a cell: walls block, diagonals cost sqrt 2.
fn step_cost(walls: &Grid<bool>) -> impl Fn(IVec2, IVec2) -> f32 + '_ {
    move |from, to| {
        if walls.get(to).copied().unwrap_or(true) {
            BLOCKED
        } else if from.x != to.x && from.y != to.y {
            std::f32::consts::SQRT_2
        } else {
            1.0
        }
    }
}

impl Game for Paths {
    fn init(ctx: &mut Context) -> GameResult<Self> {
        let store = if ctx.headless() {
            Store::at(std::env::temp_dir().join("yowie-pathfinding-example"))?
        } else {
            Store::open("rs", "yowie", "pathfinding-example")?
        };
        let mut g = Paths {
            font: ctx.gfx.default_font(),
            walls: Grid::new(COLS, ROWS),
            start: ivec2(3, 3),
            diagonal: false,
            random: Rng::new(42),
            store,
            // Every step costs at least 1, which guides the search.
            finder: Pathfinder::new(COLS, ROWS).with_min_cost(1.0),
            status: "Left-click paints walls, right-click erases. D diagonal, R new map, F5 save, F9 load."
                .into(),
        };
        g.roll();
        Ok(g)
    }

    fn update(&mut self, ctx: &mut Context) -> GameResult {
        if ctx.input.key_pressed(Key::Escape) {
            ctx.quit();
        }
        if ctx.input.key_pressed(Key::D) {
            self.diagonal = !self.diagonal;
        }
        if ctx.input.key_pressed(Key::R) {
            self.roll();
        }
        if ctx.input.key_pressed(Key::F5) {
            let file = MapFile {
                walls: self.walls.cells().to_vec(),
                start: (self.start.x, self.start.y),
            };
            self.status = match self.store.write("map", &file) {
                Ok(()) => format!("Saved to {}", self.store.path().display()),
                Err(e) => e.to_string(),
            };
        }
        if ctx.input.key_pressed(Key::F9) {
            self.status = match self.store.read::<MapFile>("map") {
                Ok(m) if m.walls.len() == self.walls.cells().len() => {
                    self.walls.cells_mut().copy_from_slice(&m.walls);
                    self.start = ivec2(m.start.0, m.start.1);
                    "Loaded the saved map".into()
                }
                Ok(_) => "The saved map has another size".into(),
                Err(e) => format!("Load failed: {e}"),
            };
        }
        let p = Self::cell_at(ctx.input.mouse());
        if self.walls.contains(p) && p != self.start {
            if ctx.input.mouse_down(MouseButton::Left) {
                self.walls.set(p, true);
            }
            if ctx.input.mouse_down(MouseButton::Right) {
                self.walls.set(p, false);
            }
        }
        if ctx.input.mouse_down(MouseButton::Middle)
            && self.walls.get(p).is_some_and(|wall| !wall)
        {
            self.start = p;
        }
        Ok(())
    }

    fn draw(&mut self, ctx: &mut Context) -> GameResult {
        let goal = if ctx.headless() {
            ivec2(COLS as i32 - 4, ROWS as i32 - 4)
        } else {
            Self::cell_at(ctx.input.mouse())
        };
        let rule = self.rule();
        let walls = &self.walls;
        let dist = dijkstra(COLS, ROWS, &[self.start], rule, step_cost(walls));
        let hood = if self.diagonal {
            Neighbourhood::Eight
        } else {
            Neighbourhood::Four
        };
        let region = flood_fill(COLS, ROWS, self.start, hood, |p| {
            walls.get(p).is_some_and(|w| !w)
        });
        let mut visible = std::collections::HashSet::new();
        fov(
            self.start,
            9,
            |p| walls.get(p).copied().unwrap_or(true),
            |p| {
                visible.insert(p);
            },
        );
        let far = dist
            .cells()
            .iter()
            .copied()
            .filter(|d| d.is_finite())
            .fold(1.0f32, f32::max);
        for (p, &wall) in walls.iter() {
            let d = dist.get(p).copied().unwrap_or(BLOCKED);
            let mut c = if wall {
                Color::rgb8(40, 40, 55)
            } else if !d.is_finite() {
                Color::rgb8(25, 25, 30)
            } else {
                let t = d / far;
                Color::rgb(0.15 + 0.7 * t, 0.35 - 0.2 * t, 0.55 - 0.45 * t)
            };
            if visible.contains(&p) {
                c = Color::rgb((c.r + 0.25).min(1.0), (c.g + 0.25).min(1.0), (c.b + 0.2).min(1.0));
            }
            let at = vec2(p.x as f32 * CELL, p.y as f32 * CELL);
            ctx.gfx.fill_rect(Rect::new(at.x + 1.0, at.y + 1.0, CELL - 2.0, CELL - 2.0), c);
        }
        let path = self
            .finder
            .find(self.start, goal, rule, step_cost(&self.walls));
        let mark = |ctx: &mut Context, p: IVec2, inset: f32, color: Color| {
            let at = vec2(p.x as f32 * CELL + inset, p.y as f32 * CELL + inset);
            ctx.gfx
                .fill_rect(Rect::new(at.x, at.y, CELL - 2.0 * inset, CELL - 2.0 * inset), color);
        };
        if let Some(path) = &path {
            for &p in &path.points {
                mark(ctx, p, 7.0, Color::rgb8(255, 235, 90));
            }
        }
        for p in line(self.start, goal) {
            let at = vec2(p.x as f32 * CELL + 10.0, p.y as f32 * CELL + 10.0);
            ctx.gfx
                .fill_rect(Rect::new(at.x, at.y, 4.0, 4.0), Color::rgba8(255, 255, 255, 120));
        }
        mark(ctx, self.start, 4.0, Color::rgb8(80, 255, 120));
        mark(ctx, goal, 4.0, Color::rgb8(255, 90, 90));

        let y = ROWS as f32 * CELL + 8.0;
        common::label(ctx, &self.font, &self.status, vec2(12.0, y), 15.0, common::TEXT);
        let steps = path.as_ref().map_or(0, |p| p.points.len().saturating_sub(1));
        let mut summary = format!(
            "Path {steps} steps ({}); {} cells reachable; {} visible; middle-click moves the start.",
            if self.diagonal { "8-way" } else { "4-way" },
            region.len(),
            visible.len()
        );
        if path.is_none() {
            summary = format!("No path to the goal. {summary}");
        }
        common::label(ctx, &self.font, &summary, vec2(12.0, y + 22.0), 15.0, common::DIM);
        Ok(())
    }
}

fn main() -> Result<(), yowie::Error> {
    let config = Config::new("Yowie pathfinding").with_size(COLS as u32 * CELL as u32, ROWS as u32 * CELL as u32 + 60);
    yowie::run::<Paths>(config)
}
