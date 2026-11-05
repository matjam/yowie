//! `--autoplay`: a player that fills in [`Controls`] each update, so the
//! headless run reaches a position with some history in it. It picks the
//! landing spot with the best score for the piece in hand (few holes, low
//! stack, cleared rows), turns the piece, slides it there and drops it.

use yowie::ecs::{Entity, World};

use crate::model::*;

/// The plan for the piece being steered.
#[derive(Default)]
pub struct Autoplay {
    piece: Option<Entity>,
    target: Option<Falling>,
    turns_left: u32,
}

impl Autoplay {
    /// The controls for this update.
    pub fn controls(&mut self, world: &World) -> Controls {
        let Some((entity, piece)) = falling(world) else {
            self.piece = None;
            return Controls::default();
        };
        let board = res::<Board>(world);
        if self.piece != Some(entity) {
            self.piece = Some(entity);
            self.target = best_landing(board, &piece);
            self.turns_left = 4;
        }
        let Some(target) = &self.target else {
            return Controls {
                drop: true,
                ..Controls::default()
            };
        };
        let mut c = Controls::default();
        if piece.cells != target.cells && self.turns_left > 0 {
            self.turns_left -= 1;
            c.rotate = true;
        } else if piece.x < target.x {
            c.right = true;
        } else if piece.x > target.x {
            c.left = true;
        } else {
            c.drop = true;
        }
        c
    }
}

/// The landing of `piece` over every turn and column that scores best.
fn best_landing(board: &Board, piece: &Falling) -> Option<Falling> {
    let mut best: Option<(f32, Falling)> = None;
    let mut turned = piece.clone();
    for _ in 0..4 {
        for x in -3..COLS as i32 {
            let candidate = Falling {
                x,
                ..turned.clone()
            };
            if !fits(board, &candidate) {
                continue;
            }
            let landed = landing(board, &candidate);
            let score = evaluate(board, &landed);
            if best.as_ref().is_none_or(|(s, _)| score > *s) {
                best = Some((score, candidate));
            }
        }
        turned = turned.rotated();
    }
    best.map(|(_, p)| p)
}

/// How good the board is after `landed` settles: higher is better.
fn evaluate(board: &Board, landed: &Falling) -> f32 {
    let mut b = *board;
    for (x, y) in landed.blocks().filter(|(_, y)| *y >= 0) {
        b.full[y as usize][x as usize] = true;
    }
    let lines = b.full.iter().filter(|r| r.iter().all(|c| *c)).count();
    let mut heights = [0_i32; COLS];
    let mut holes = 0;
    for x in 0..COLS {
        let top = (0..ROWS).find(|y| b.full[*y][x]);
        if let Some(top) = top {
            heights[x] = (ROWS - top) as i32;
            holes += (top..ROWS).filter(|y| !b.full[*y][x]).count();
        }
    }
    let total: i32 = heights.iter().sum();
    let bumpiness: i32 = heights.windows(2).map(|w| (w[0] - w[1]).abs()).sum();
    -0.51 * total as f32 + 0.76 * lines as f32 - 0.36 * holes as f32 - 0.18 * bumpiness as f32
}
