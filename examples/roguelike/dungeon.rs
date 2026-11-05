//! The dungeon: rooms and corridors, actors, field of view and goblin
//! movement, built on `yowie-grid` and `yowie-rng`.

use yowie::grid::{BLOCKED, Diagonal, Grid, Pathfinder, fov};
use yowie::rng::Rng;
use yowie::{IVec2, ivec2};

pub const MAP_W: usize = 60;
pub const MAP_H: usize = 32;
const SIGHT: i32 = 8;

#[derive(Clone, Copy, Default)]
pub struct Tile {
    pub wall: bool,
    pub seen: bool,
    pub visible: bool,
}

pub struct Actor {
    pub pos: IVec2,
    pub hp: i32,
    pub max_hp: i32,
}

impl Actor {
    fn new(pos: IVec2, hp: i32) -> Self {
        Self {
            pos,
            hp,
            max_hp: hp,
        }
    }
}

pub struct Dungeon {
    pub tiles: Grid<Tile>,
    pub player: Actor,
    pub monsters: Vec<Actor>,
    finder: Pathfinder,
}

struct Room {
    x: i32,
    y: i32,
    w: i32,
    h: i32,
}

impl Room {
    fn center(&self) -> IVec2 {
        ivec2(self.x + self.w / 2, self.y + self.h / 2)
    }

    fn overlaps(&self, o: &Room) -> bool {
        self.x <= o.x + o.w && o.x <= self.x + self.w && self.y <= o.y + o.h && o.y <= self.y + self.h
    }
}

impl Dungeon {
    /// Carves random rooms joined by L-shaped corridors and places the
    /// player in the first room and goblins in most of the others.
    pub fn new(rng: &mut Rng) -> Self {
        let mut tiles = Grid::filled(
            MAP_W,
            MAP_H,
            Tile {
                wall: true,
                ..Tile::default()
            },
        );
        let mut rooms: Vec<Room> = Vec::new();
        for _ in 0..40 {
            let r = Room {
                x: 1 + rng.range(0, MAP_W as i32 - 12),
                y: 1 + rng.range(0, MAP_H as i32 - 8),
                w: 4 + rng.range(0, 8),
                h: 3 + rng.range(0, 5),
            };
            if r.x + r.w >= MAP_W as i32 - 1
                || r.y + r.h >= MAP_H as i32 - 1
                || rooms.iter().any(|o| r.overlaps(o))
            {
                continue;
            }
            carve(&mut tiles, r.x, r.y, r.x + r.w - 1, r.y + r.h - 1);
            if let Some(prev) = rooms.last() {
                let (p, c) = (prev.center(), r.center());
                if rng.chance(0.5) {
                    carve(&mut tiles, p.x.min(c.x), p.y, p.x.max(c.x), p.y);
                    carve(&mut tiles, c.x, p.y.min(c.y), c.x, p.y.max(c.y));
                } else {
                    carve(&mut tiles, p.x, p.y.min(c.y), p.x, p.y.max(c.y));
                    carve(&mut tiles, p.x.min(c.x), c.y, p.x.max(c.x), c.y);
                }
            }
            rooms.push(r);
        }
        let start = rooms.first().map_or(ivec2(1, 1), Room::center);
        let monsters = rooms
            .iter()
            .skip(1)
            .filter(|_| rng.chance(2.0 / 3.0))
            .map(|r| Actor::new(r.center(), 3))
            .collect();
        let mut d = Dungeon {
            tiles,
            player: Actor::new(start, 10),
            monsters,
            finder: Pathfinder::new(MAP_W, MAP_H).with_min_cost(1.0),
        };
        d.compute_fov();
        d
    }

    fn is_wall(&self, p: IVec2) -> bool {
        self.tiles.get(p).is_none_or(|t| t.wall)
    }

    fn monster_at(&self, p: IVec2) -> Option<usize> {
        self.monsters.iter().position(|m| m.hp > 0 && m.pos == p)
    }

    /// Moves the player by `d`, or attacks a goblin standing there. Returns
    /// a message when something happened.
    pub fn move_player(&mut self, d: IVec2) -> Option<String> {
        let to = self.player.pos + d;
        if self.is_wall(to) {
            return None;
        }
        if let Some(i) = self.monster_at(to) {
            let m = &mut self.monsters[i];
            m.hp -= 2;
            return Some(if m.hp <= 0 {
                "You slay the goblin.".into()
            } else {
                "You hit the goblin.".into()
            });
        }
        self.player.pos = to;
        None
    }

    /// Lets every goblin the player can see act: bite when adjacent,
    /// otherwise step along the shortest path to the player.
    pub fn monsters_act(&mut self) -> Vec<String> {
        let mut messages = Vec::new();
        for i in 0..self.monsters.len() {
            let pos = self.monsters[i].pos;
            let visible = self.tiles.get(pos).is_some_and(|t| t.visible);
            if self.monsters[i].hp <= 0 || !visible {
                continue;
            }
            let delta = (self.player.pos - pos).abs();
            if delta.x <= 1 && delta.y <= 1 {
                self.player.hp -= 1;
                messages.push("The goblin bites you!".into());
                continue;
            }
            let others: Vec<IVec2> = self
                .monsters
                .iter()
                .enumerate()
                .filter(|(j, m)| *j != i && m.hp > 0)
                .map(|(_, m)| m.pos)
                .collect();
            let (tiles, player) = (&self.tiles, self.player.pos);
            let path = self.finder.find(pos, player, Diagonal::IfBothClear, |_, to| {
                if tiles.get(to).is_none_or(|t| t.wall) || others.contains(&to) {
                    BLOCKED
                } else {
                    1.0
                }
            });
            if let Some(next) = path.and_then(|p| p.points.get(1).copied())
                && next != player
            {
                self.monsters[i].pos = next;
            }
        }
        messages
    }

    /// The number of goblins still alive.
    pub fn alive(&self) -> usize {
        self.monsters.iter().filter(|m| m.hp > 0).count()
    }

    /// Recomputes what the player sees within eight cells with symmetric
    /// shadowcasting, and remembers it as seen.
    pub fn compute_fov(&mut self) {
        for t in self.tiles.cells_mut() {
            t.visible = false;
        }
        let tiles = self.tiles.clone();
        let mut lit = Vec::new();
        fov(
            self.player.pos,
            SIGHT,
            |p| tiles.get(p).is_none_or(|t| t.wall),
            |p| lit.push(p),
        );
        for p in lit {
            if let Some(t) = self.tiles.get_mut(p) {
                t.visible = true;
                t.seen = true;
            }
        }
    }

    /// The next step of a shortest path from the player to the nearest
    /// living goblin, for the demo's autoplay.
    pub fn step_toward_nearest_goblin(&mut self) -> Option<IVec2> {
        let tiles = &self.tiles;
        let from = self.player.pos;
        let best = self
            .monsters
            .iter()
            .filter(|m| m.hp > 0)
            .filter_map(|m| {
                self.finder
                    .find(from, m.pos, Diagonal::IfBothClear, |_, to| {
                        if tiles.get(to).is_none_or(|t| t.wall) {
                            BLOCKED
                        } else {
                            1.0
                        }
                    })
            })
            .min_by(|a, b| a.cost.total_cmp(&b.cost))?;
        best.points.get(1).map(|&next| next - from)
    }
}

fn carve(tiles: &mut Grid<Tile>, x0: i32, y0: i32, x1: i32, y1: i32) {
    for y in y0..=y1 {
        for x in x0..=x1 {
            if let Some(t) = tiles.get_mut(ivec2(x, y)) {
                t.wall = false;
            }
        }
    }
}
