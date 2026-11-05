//! Step 1 of the Tetris walk-through: what exists in the game. Components
//! are data on entities, resources are the singletons the systems share,
//! events tell systems what happened, and the helper functions answer
//! questions about pieces and the board.

use yowie::Color;
use yowie::ecs::{Entity, World};
use yowie::rng::Rng;
use yowie::timer::{TimerId, Timers};
use yowie::tween::Tween;

/// The board is ten cells wide and twenty tall.
pub const COLS: usize = 10;
/// See [`COLS`].
pub const ROWS: usize = 20;

/// One kind of piece: its name, its shape as text (`#` is a filled cell)
/// and its colour.
pub struct Shape {
    pub name: &'static str,
    pub rows: &'static [&'static str],
    pub color: Color,
}

/// The seven pieces. The game rotates the shape, so each is listed once.
pub const SHAPES: [Shape; 7] = [
    Shape {
        name: "I",
        rows: &["....", "####", "....", "...."],
        color: Color::hex(0x5BC0EB),
    },
    Shape {
        name: "O",
        rows: &["##", "##"],
        color: Color::hex(0xFDE74C),
    },
    Shape {
        name: "T",
        rows: &[".#.", "###", "..."],
        color: Color::hex(0x9B5DE5),
    },
    Shape {
        name: "S",
        rows: &[".##", "##.", "..."],
        color: Color::hex(0x9BC53D),
    },
    Shape {
        name: "Z",
        rows: &["##.", ".##", "..."],
        color: Color::hex(0xE55934),
    },
    Shape {
        name: "J",
        rows: &["#..", "###", "..."],
        color: Color::hex(0x3A86FF),
    },
    Shape {
        name: "L",
        rows: &["..#", "###", "..."],
        color: Color::hex(0xFA7921),
    },
];

// Components: data attached to entities.

/// One settled block on the board.
#[derive(Clone, Copy, Debug)]
pub struct Cell {
    pub x: i32,
    pub y: i32,
    pub kind: usize,
}

/// The piece the player steers. There is one at a time.
#[derive(Clone, Debug, PartialEq)]
pub struct Falling {
    pub kind: usize,
    /// `cells[y][x]` is true where the piece has a block.
    pub cells: Vec<Vec<bool>>,
    pub x: i32,
    pub y: i32,
}

/// A white wash over one board row that fades out; a tween drives it.
pub struct RowFlash {
    pub row: i32,
    pub fade: Tween<f32>,
}

// Resources: singletons stored on the world.

/// Which board cells are occupied. The `board` system rebuilds it from the
/// [`Cell`] entities so the other systems test collisions against an array.
#[derive(Clone, Copy)]
pub struct Board {
    pub full: [[bool; COLS]; ROWS],
}

impl Default for Board {
    fn default() -> Self {
        Board {
            full: [[false; COLS]; ROWS],
        }
    }
}

/// What the score panel shows.
#[derive(Clone, Copy, Debug, Default)]
pub struct Score {
    pub points: u32,
    pub lines: u32,
    pub level: u32,
    pub over: bool,
}

/// The keys pressed this update, copied from the engine's input state so the
/// systems read only the world.
#[derive(Clone, Copy, Debug, Default)]
pub struct Controls {
    pub left: bool,
    pub right: bool,
    pub rotate: bool,
    pub down: bool,
    pub drop: bool,
}

/// Seconds the current update covers, copied from `ctx.time`.
pub struct Dt(pub f32);

/// What a timer in [`Clock`] means when it fires.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tick {
    /// The piece falls one row.
    Gravity,
    /// A piece that rested on the stack for the lock delay settles.
    Lock,
}

/// Gravity and lock delay run on game time, so pausing or slowing the game
/// slows them too.
pub struct Clock {
    pub timers: Timers<Tick>,
    pub gravity: TimerId,
    /// Running while the piece rests on something.
    pub lock: Option<TimerId>,
}

/// Deals the pieces.
pub struct Bag {
    pub rng: Rng,
    pub next: usize,
}

// Events: how systems tell each other what happened. Each one is read and
// then cleared by the system that consumes it.

/// Asks the `lock` system to settle the falling piece.
#[derive(Clone)]
pub struct LockPiece;
/// A piece settled.
#[derive(Clone)]
pub struct Locked;
/// Rows were removed.
#[derive(Clone)]
pub struct Cleared {
    pub rows: Vec<i32>,
}
/// The stack reached the top.
#[derive(Clone)]
pub struct GameOver;

/// Builds a piece at the top of the board.
pub fn new_falling(kind: usize) -> Falling {
    let rows = SHAPES[kind].rows;
    Falling {
        kind,
        cells: rows
            .iter()
            .map(|r| r.chars().map(|c| c == '#').collect())
            .collect(),
        x: (COLS - rows[0].len()) as i32 / 2,
        y: 0,
    }
}

impl Falling {
    /// The piece turned clockwise: transpose and flip over a square grid.
    pub fn rotated(&self) -> Falling {
        let n = self.cells.len();
        let cells = (0..n)
            .map(|y| (0..n).map(|x| self.cells[n - 1 - x][y]).collect())
            .collect();
        Falling {
            cells,
            ..self.clone()
        }
    }

    /// The board cells the piece covers.
    pub fn blocks(&self) -> impl Iterator<Item = (i32, i32)> + '_ {
        self.cells.iter().enumerate().flat_map(move |(y, row)| {
            row.iter()
                .enumerate()
                .filter(|(_, on)| **on)
                .map(move |(x, _)| (self.x + x as i32, self.y + y as i32))
        })
    }

    /// The piece moved by `(dx, dy)`.
    pub fn moved(&self, dx: i32, dy: i32) -> Falling {
        Falling {
            x: self.x + dx,
            y: self.y + dy,
            ..self.clone()
        }
    }
}

/// Whether the piece stays inside the walls and floor and off the stack.
pub fn fits(board: &Board, piece: &Falling) -> bool {
    piece.blocks().all(|(x, y)| {
        (0..COLS as i32).contains(&x)
            && y < ROWS as i32
            && (y < 0 || !board.full[y as usize][x as usize])
    })
}

/// Where the piece would land if it dropped now.
pub fn landing(board: &Board, piece: &Falling) -> Falling {
    let mut p = piece.clone();
    while fits(board, &p.moved(0, 1)) {
        p = p.moved(0, 1);
    }
    p
}

// Small world helpers, so systems read as rules rather than lookups.

/// A resource the game inserted when it started.
pub fn res<T: 'static>(world: &World) -> &T {
    world
        .resource::<T>()
        .expect("Tetris::reset inserts every resource before the schedule runs")
}

/// See [`res`].
pub fn res_mut<T: 'static>(world: &mut World) -> &mut T {
    world
        .resource_mut::<T>()
        .expect("Tetris::reset inserts every resource before the schedule runs")
}

/// The falling piece and its entity, if there is one.
pub fn falling(world: &World) -> Option<(Entity, Falling)> {
    world
        .query::<(Entity, &Falling)>()
        .iter()
        .next()
        .map(|(e, p)| (e, p.clone()))
}

/// Reads the events of type `T` and clears them, so a system that runs on
/// the next update does not see them again (events stay readable for two
/// updates).
pub fn take_events<T: Clone + 'static>(world: &mut World) -> Vec<T> {
    let events = world.read_events::<T>().cloned().collect();
    world.clear_events::<T>();
    events
}
