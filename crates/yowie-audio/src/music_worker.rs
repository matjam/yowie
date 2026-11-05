//! The decoder thread behind [`Music`](crate::Music).

use crate::decode::Decoder;
use crate::error::Error;
use crate::music::{BLOCK_FRAMES, Block, NO_SEEK, Shared};
use crossbeam_channel::{Receiver, Sender, TrySendError};
use std::sync::atomic::Ordering;
use std::sync::{Arc, PoisonError};
use std::time::Duration;

/// A streaming linear resampler that carries its phase between calls.
pub(crate) struct Resampler {
    step: f64,
    pos: f64,
    prev: [f32; 2],
    have: bool,
}

impl Resampler {
    pub(crate) fn new(from: u32, to: u32) -> Self {
        Self {
            step: f64::from(from) / f64::from(to),
            pos: 0.0,
            prev: [0.0; 2],
            have: false,
        }
    }

    fn reset(&mut self) {
        self.pos = 0.0;
        self.have = false;
    }

    pub(crate) fn push(&mut self, input: &[f32], out: &mut Vec<f32>) {
        if (self.step - 1.0).abs() < 1e-9 {
            out.extend_from_slice(input);
            return;
        }
        for f in input.chunks_exact(2) {
            if !self.have {
                self.prev = [f[0], f[1]];
                self.have = true;
                self.pos = 0.0;
                continue;
            }
            while self.pos < 1.0 {
                let t = self.pos as f32;
                out.push(self.prev[0] + (f[0] - self.prev[0]) * t);
                out.push(self.prev[1] + (f[1] - self.prev[1]) * t);
                self.pos += self.step;
            }
            self.pos -= 1.0;
            self.prev = [f[0], f[1]];
        }
    }

    pub(crate) fn finish(&mut self, out: &mut Vec<f32>) {
        if self.have && self.step != 1.0 {
            out.extend_from_slice(&self.prev);
        }
    }
}

/// Decodes, resamples and queues blocks until the music is closed.
pub(crate) struct Worker {
    dec: Decoder,
    shared: Arc<Shared>,
    tx: Sender<Block>,
    recycle: Receiver<Vec<f32>>,
    rs: Resampler,
    generation: u64,
    src_pos: u64,
    out: Vec<f32>,
    stereo: Vec<f32>,
    packet: Vec<f32>,
}

enum Flow {
    Continue,
    Stop,
}

impl Worker {
    pub(crate) fn new(
        dec: Decoder,
        rate: u32,
        shared: Arc<Shared>,
        tx: Sender<Block>,
        recycle: Receiver<Vec<f32>>,
    ) -> Self {
        let rs = Resampler::new(dec.rate, rate);
        Self {
            dec,
            shared,
            tx,
            recycle,
            rs,
            generation: 0,
            src_pos: 0,
            out: Vec::new(),
            stereo: Vec::new(),
            packet: Vec::new(),
        }
    }

    fn fail(&self, e: &Error) {
        *self
            .shared
            .error
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(e.to_string());
        self.shared.ended.store(true, Ordering::Release);
    }

    fn loop_frames(&self) -> (u64, u64) {
        let rate = f64::from(self.dec.rate);
        let a = f64::from_bits(self.shared.loop_start.load(Ordering::Acquire));
        let b = f64::from_bits(self.shared.loop_end.load(Ordering::Acquire));
        ((a * rate) as u64, (b * rate) as u64)
    }

    /// Applies a pending seek.
    fn check_seek(&mut self) {
        let generation = self.shared.generation.load(Ordering::Acquire);
        if generation == self.generation {
            return;
        }
        self.generation = generation;
        self.out.clear();
        self.rs.reset();
        self.shared.ended.store(false, Ordering::Release);
        let bits = self.shared.seek_to.swap(NO_SEEK, Ordering::AcqRel);
        if bits != NO_SEEK {
            let s = f64::from_bits(bits);
            match self.dec.seek(s) {
                Ok(()) => self.src_pos = (s * f64::from(self.dec.rate)) as u64,
                Err(e) => log::warn!("music seek failed: {e}"),
            }
        }
    }

    pub(crate) fn run(mut self) {
        loop {
            if self.shared.closed.load(Ordering::Acquire) {
                return;
            }
            self.check_seek();
            if self.shared.ended.load(Ordering::Acquire) {
                std::thread::sleep(Duration::from_millis(5));
                continue;
            }
            if let Flow::Stop = self.step() {
                return;
            }
        }
    }

    /// Decodes one packet, or handles the end of the file.
    fn step(&mut self) -> Flow {
        self.packet.clear();
        let more = match self.dec.read_more(&mut self.packet) {
            Ok(m) => m,
            Err(e) => {
                self.fail(&e);
                self.flush(true);
                return Flow::Stop;
            }
        };
        if more {
            let (ls, le) = self.loop_frames();
            let looping = self.shared.looping.load(Ordering::Acquire);
            self.fill_stereo();
            let mut frames = self.stereo.len() / 2;
            let mut hit_end = false;
            if looping && le > ls && self.src_pos + frames as u64 >= le {
                frames = le.saturating_sub(self.src_pos) as usize;
                self.stereo.truncate(frames * 2);
                hit_end = true;
            }
            self.src_pos += frames as u64;
            self.rs.push(&self.stereo, &mut self.out);
            if !self.drain(false) {
                return Flow::Continue;
            }
            if hit_end {
                return self.rewind();
            }
            return Flow::Continue;
        }
        if self.shared.looping.load(Ordering::Acquire) {
            return self.rewind();
        }
        self.rs.finish(&mut self.out);
        self.drain(true);
        self.shared.ended.store(true, Ordering::Release);
        Flow::Continue
    }

    /// Returns to the loop start.
    fn rewind(&mut self) -> Flow {
        let (ls, _) = self.loop_frames();
        let start = ls as f64 / f64::from(self.dec.rate);
        match self.dec.seek(start) {
            Ok(()) => {
                self.src_pos = ls;
                Flow::Continue
            }
            Err(e) => {
                self.fail(&e);
                self.flush(true);
                Flow::Stop
            }
        }
    }

    fn fill_stereo(&mut self) {
        self.stereo.clear();
        if self.dec.channels == 1 {
            self.stereo
                .extend(self.packet.iter().flat_map(|s| [*s, *s]));
        } else {
            self.stereo.extend_from_slice(&self.packet);
        }
    }

    fn flush(&mut self, last: bool) {
        self.drain(last);
        self.shared.ended.store(true, Ordering::Release);
    }

    /// Sends full blocks (and the remainder when `last`). Returns false when a seek
    /// arrived while waiting for room, in which case the pending audio is discarded.
    fn drain(&mut self, last: bool) -> bool {
        let block = BLOCK_FRAMES * 2;
        while self.out.len() >= block || last {
            let n = self.out.len().min(block);
            let mut data = self.recycle.try_recv().unwrap_or_default();
            data.clear();
            data.extend_from_slice(&self.out[..n]);
            self.out.drain(..n);
            let is_last = last && self.out.is_empty();
            let mut b = Block {
                generation: self.generation,
                data,
                last: is_last,
            };
            loop {
                if self.shared.generation.load(Ordering::Acquire) != self.generation
                    || self.shared.closed.load(Ordering::Acquire)
                {
                    return false;
                }
                match self.tx.try_send(b) {
                    Ok(()) => break,
                    Err(TrySendError::Full(back)) => {
                        b = back;
                        // Wait for the audio thread to consume a block.
                        std::thread::sleep(Duration::from_millis(2));
                    }
                    Err(TrySendError::Disconnected(_)) => return false,
                }
            }
            if is_last {
                break;
            }
        }
        true
    }
}
