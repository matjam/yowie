//! Steps 2 to 4 of the Tetris walk-through: the systems and the schedule
//! that orders them. Every system is a function of the world; input and
//! the clock arrive as resources, and results leave as events.

use yowie::audio::{Mixer, PlayOptions, Sound};
use yowie::ecs::{Entity, Schedule, Stage, System, World};
use yowie::tween::{Curve, Ease, Tween};

use crate::model::*;

/// Seconds a resting piece may still be moved before it locks.
const LOCK_DELAY: f64 = 0.5;
/// Points for clearing 0 to 4 rows at once, before the level bonus.
const LINE_POINTS: [u32; 5] = [0, 100, 300, 500, 800];
/// Rows to clear for each level.
const LINES_PER_LEVEL: u32 = 10;

/// Seconds between gravity steps at `level`.
pub fn gravity_interval(level: u32) -> f64 {
    (0.6 * 0.85_f64.powi(level as i32)).max(0.08)
}

/// The sounds and the mixer the effects system plays them on.
pub struct Effects {
    pub mixer: Mixer,
    pub lock: Sound,
    pub clear: Sound,
    pub rotate: Sound,
    pub over: Sound,
}

/// Asks `lock` to settle the piece the player turned; the `effects` system
/// plays the click.
#[derive(Clone)]
pub struct Rotated;

/// Builds the schedule.
///
/// Systems are named, and `before` and `after` say which must run first
/// within a stage. The order they are added in does not matter, so the
/// effects system can be listed first and still run last. The rules share
/// a set, `rules`, so that one `after("rules")` covers all of them.
pub fn schedule(effects: Effects) -> Schedule {
    let mut s = Schedule::new();
    s.add(
        Stage::Update,
        System::new("effects", effects_system(effects)).after("rules"),
    );
    s.add(
        Stage::Update,
        System::new("board", rebuild_board_system).in_set("rules"),
    );
    s.add(
        Stage::Update,
        System::new("input", input)
            .in_set("rules")
            .after("board")
            .run_if(playing),
    );
    s.add(
        Stage::Update,
        System::new("lock-delay", lock_delay)
            .in_set("rules")
            .after("input")
            .run_if(playing),
    );
    s.add(
        Stage::Update,
        System::new("gravity", gravity)
            .in_set("rules")
            .after("lock-delay")
            .run_if(playing),
    );
    s.add(
        Stage::Update,
        System::new("lock", lock).in_set("rules").after("gravity"),
    );
    s.add(
        Stage::Update,
        System::new("clear-lines", clear_lines)
            .in_set("rules")
            .after("lock"),
    );
    s.add(
        Stage::Update,
        System::new("spawn", spawn)
            .in_set("rules")
            .after("clear-lines")
            .run_if(playing),
    );
    s
}

/// Run condition: the game is not over.
fn playing(world: &World) -> bool {
    !res::<Score>(world).over
}

/// Rebuilds the occupancy grid from the [`Cell`] entities.
pub fn rebuild_board(world: &mut World) {
    let mut board = Board::default();
    for c in world.query::<&Cell>().iter() {
        if (0..ROWS as i32).contains(&c.y) && (0..COLS as i32).contains(&c.x) {
            board.full[c.y as usize][c.x as usize] = true;
        }
    }
    *res_mut::<Board>(world) = board;
}

fn rebuild_board_system(world: &mut World) {
    rebuild_board(world);
}

/// Moves `piece` to `candidate` if it fits.
fn attempt(board: &Board, piece: &mut Falling, candidate: Falling) -> bool {
    let ok = fits(board, &candidate);
    if ok {
        *piece = candidate;
    }
    ok
}

/// Moves `piece` by `(dx, dy)` if the new place is free.
fn shift(board: &Board, piece: &mut Falling, dx: i32, dy: i32) -> bool {
    let candidate = piece.moved(dx, dy);
    attempt(board, piece, candidate)
}

/// Applies the [`Controls`] to the falling piece.
fn input(world: &mut World) {
    let controls = *res::<Controls>(world);
    let board = *res::<Board>(world);
    let Some((entity, mut piece)) = falling(world) else {
        return;
    };
    let mut points = 0;
    if controls.left {
        shift(&board, &mut piece, -1, 0);
    }
    if controls.right {
        shift(&board, &mut piece, 1, 0);
    }
    if controls.rotate {
        let turned = piece.rotated();
        // Wall kick: nudge sideways when the turn would clip a wall.
        let turned_ok = [0, -1, 1, -2, 2]
            .into_iter()
            .any(|dx| attempt(&board, &mut piece, turned.moved(dx, 0)));
        if turned_ok {
            world.send_event(Rotated);
        }
    }
    if controls.down && shift(&board, &mut piece, 0, 1) {
        points += 1;
    }
    if controls.drop {
        let landed = landing(&board, &piece);
        points += 2 * (landed.y - piece.y) as u32;
        piece = landed;
        world.send_event(LockPiece);
    }
    if let Ok(mut stored) = world.get::<&mut Falling>(entity) {
        *stored = piece;
    }
    res_mut::<Score>(world).points += points;
}

/// Starts the lock timer when the piece rests on something and cancels it
/// when the piece can fall again, so a piece may slide a little before it
/// settles.
fn lock_delay(world: &mut World) {
    let Some((_, piece)) = falling(world) else {
        return;
    };
    let resting = !fits(res::<Board>(world), &piece.moved(0, 1));
    let clock = res_mut::<Clock>(world);
    match (resting, clock.lock) {
        (true, None) => clock.lock = Some(clock.timers.after(LOCK_DELAY, Tick::Lock)),
        (false, Some(id)) => {
            clock.timers.cancel(id);
            clock.lock = None;
        }
        _ => {}
    }
}

/// Advances the clock by the update's game time and reacts to the timers
/// that fired.
fn gravity(world: &mut World) {
    let dt = f64::from(res::<Dt>(world).0);
    let fired = res_mut::<Clock>(world).timers.update(dt);
    for f in fired {
        match f.data {
            Tick::Gravity => {
                let board = *res::<Board>(world);
                if let Some((entity, piece)) = falling(world)
                    && fits(&board, &piece.moved(0, 1))
                    && let Ok(mut stored) = world.get::<&mut Falling>(entity)
                {
                    stored.y += 1;
                }
            }
            Tick::Lock => {
                res_mut::<Clock>(world).lock = None;
                world.send_event(LockPiece);
            }
        }
    }
}

/// Turns the falling piece into [`Cell`] entities when something asked
/// for it.
fn lock(world: &mut World) {
    if take_events::<LockPiece>(world).is_empty() {
        return;
    }
    let Some((entity, piece)) = falling(world) else {
        return;
    };
    for (x, y) in piece.blocks().filter(|(_, y)| *y >= 0) {
        world.spawn((Cell {
            x,
            y,
            kind: piece.kind,
        },));
    }
    // A missing entity means the piece is already gone.
    let _ = world.despawn(entity);
    let clock = res_mut::<Clock>(world);
    if let Some(id) = clock.lock.take() {
        clock.timers.cancel(id);
    }
    world.send_event(Locked);
}

/// Removes full rows, lets the cells above fall, and scores them.
fn clear_lines(world: &mut World) {
    rebuild_board(world);
    let board = *res::<Board>(world);
    let full: Vec<i32> = (0..ROWS)
        .filter(|y| board.full[*y].iter().all(|c| *c))
        .map(|y| y as i32)
        .collect();
    if full.is_empty() {
        return;
    }
    let mut doomed = Vec::new();
    for (entity, cell) in world.query_mut::<(Entity, &mut Cell)>() {
        if full.contains(&cell.y) {
            doomed.push(entity);
        } else {
            // Each cleared row below the cell moves it down one.
            cell.y += full.iter().filter(|row| **row > cell.y).count() as i32;
        }
    }
    for entity in doomed {
        let _ = world.despawn(entity);
    }
    rebuild_board(world);

    let score = res_mut::<Score>(world);
    score.points += LINE_POINTS[full.len().min(4)] * (score.level + 1);
    score.lines += full.len() as u32;
    let level = score.lines / LINES_PER_LEVEL;
    if level != score.level {
        score.level = level;
        // A new level replaces the gravity timer with a faster one.
        let clock = res_mut::<Clock>(world);
        clock.timers.cancel(clock.gravity);
        clock.gravity = clock.timers.every(gravity_interval(level), Tick::Gravity);
    }
    world.send_event(Cleared { rows: full });
}

/// Deals the next piece. If it does not fit the game is over.
fn spawn(world: &mut World) {
    if falling(world).is_some() {
        return;
    }
    let bag = res_mut::<Bag>(world);
    let kind = bag.next;
    bag.next = bag.rng.below(SHAPES.len() as u64) as usize;
    let piece = new_falling(kind);
    if !fits(res::<Board>(world), &piece) {
        res_mut::<Score>(world).over = true;
        world.send_event(GameOver);
    }
    world.spawn((piece,));
}

/// Turns events into sound and into the row flashes tweens drive. It runs
/// after every rule, so it sees what they sent in the same update.
fn effects_system(fx: Effects) -> impl FnMut(&mut World) {
    move |world| {
        let play = |sound: &Sound, volume: f32, pitch: f32| {
            fx.mixer.play(
                sound,
                PlayOptions {
                    volume,
                    pitch,
                    ..PlayOptions::default()
                },
            );
        };
        for _ in take_events::<Rotated>(world) {
            play(&fx.rotate, 0.25, 1.0);
        }
        for _ in take_events::<Locked>(world) {
            play(&fx.lock, 0.4, 1.0);
        }
        for cleared in take_events::<Cleared>(world) {
            play(&fx.clear, 0.5, 1.0 + 0.2 * cleared.rows.len() as f32);
            for row in cleared.rows {
                world.spawn((RowFlash {
                    row,
                    fade: Tween::new(1.0, 0.0, 0.4).ease(Ease::Out(Curve::Quad)),
                },));
            }
        }
        for _ in take_events::<GameOver>(world) {
            play(&fx.over, 0.5, 1.0);
        }

        // The flashes fade on game time and remove themselves when done.
        let dt = res::<Dt>(world).0;
        let mut finished = Vec::new();
        for (entity, flash) in world.query_mut::<(Entity, &mut RowFlash)>() {
            flash.fade.update(dt);
            if flash.fade.done() {
                finished.push(entity);
            }
        }
        for entity in finished {
            let _ = world.despawn(entity);
        }
    }
}
