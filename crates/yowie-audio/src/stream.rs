//! Sources of audio that are produced while they play.

/// A source of interleaved stereo audio at the mixer's sample rate.
///
/// A stream is moved to the audio thread when it starts playing, so [`Stream::read`]
/// must not block, allocate in steady state or take a lock another thread can hold for
/// long. [`Music`](crate::Music) satisfies this by decoding on a background thread.
pub trait Stream: Send + 'static {
    /// Fills `out` with interleaved stereo frames and returns how many frames it wrote.
    ///
    /// Returning fewer frames than `out.len() / 2` ends the stream. A stream that falls
    /// behind should write silence for the missing frames and report them as written.
    fn read(&mut self, out: &mut [f32]) -> usize;

    /// Moves to `seconds` from the start. Returns false when the stream cannot seek.
    fn seek(&mut self, _seconds: f64) -> bool {
        false
    }
}

const BUF_FRAMES: usize = 1024;

/// The mixer's side of a playing stream: a buffer and a linear resampler that applies
/// the voice's pitch.
pub(crate) struct StreamState {
    stream: Box<dyn Stream>,
    buf: Vec<f32>,
    rd: usize,
    cnt: usize,
    eof: bool,
    a: [f32; 2],
    b: [f32; 2],
    have_b: bool,
    started: bool,
    phase: f64,
}

impl StreamState {
    pub(crate) fn new(stream: Box<dyn Stream>) -> Self {
        Self {
            stream,
            buf: vec![0.0; BUF_FRAMES * 2],
            rd: 0,
            cnt: 0,
            eof: false,
            a: [0.0; 2],
            b: [0.0; 2],
            have_b: false,
            started: false,
            phase: 0.0,
        }
    }

    fn next_frame(&mut self) -> Option<[f32; 2]> {
        if self.rd == self.cnt {
            if self.eof {
                return None;
            }
            self.rd = 0;
            self.cnt = self.stream.read(&mut self.buf).min(BUF_FRAMES);
            self.eof = self.cnt < BUF_FRAMES;
            if self.cnt == 0 {
                return None;
            }
        }
        let f = [self.buf[self.rd * 2], self.buf[self.rd * 2 + 1]];
        self.rd += 1;
        Some(f)
    }

    pub(crate) fn seek(&mut self, seconds: f64) {
        if self.stream.seek(seconds) {
            self.rd = 0;
            self.cnt = 0;
            self.eof = false;
            self.started = false;
            self.phase = 0.0;
        }
    }

    /// Writes up to `dst.len() / 2` frames advancing `step` source frames per output
    /// frame. Returns the frames written and the source frames consumed; fewer frames
    /// than asked for means the stream ended.
    pub(crate) fn read(&mut self, dst: &mut [f32], step: f64) -> (usize, f64) {
        if !self.started {
            let Some(a) = self.next_frame() else {
                return (0, 0.0);
            };
            self.a = a;
            match self.next_frame() {
                Some(b) => {
                    self.b = b;
                    self.have_b = true;
                }
                None => self.have_b = false,
            }
            self.started = true;
        }
        let frames = dst.len() / 2;
        let mut consumed = 0.0;
        for i in 0..frames {
            while self.phase >= 1.0 {
                if !self.have_b {
                    return (i, consumed);
                }
                self.a = self.b;
                match self.next_frame() {
                    Some(b) => self.b = b,
                    None => self.have_b = false,
                }
                self.phase -= 1.0;
            }
            let b = if self.have_b { self.b } else { self.a };
            let f = self.phase as f32;
            dst[i * 2] = self.a[0] + (b[0] - self.a[0]) * f;
            dst[i * 2 + 1] = self.a[1] + (b[1] - self.a[1]) * f;
            self.phase += step;
            consumed += step;
        }
        (frames, consumed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Ramp {
        n: usize,
        i: usize,
    }

    impl Stream for Ramp {
        fn read(&mut self, out: &mut [f32]) -> usize {
            let want = out.len() / 2;
            let k = want.min(self.n - self.i);
            for f in 0..k {
                out[f * 2] = (self.i + f) as f32;
                out[f * 2 + 1] = -((self.i + f) as f32);
            }
            self.i += k;
            k
        }
    }

    #[test]
    fn unity_step_copies_and_ends() {
        let mut s = StreamState::new(Box::new(Ramp { n: 3000, i: 0 }));
        let mut out = vec![0.0; 4000 * 2];
        let (n, _) = s.read(&mut out, 1.0);
        assert_eq!(n, 3000);
        assert_eq!(out[2999 * 2], 2999.0);
        assert_eq!(out[2999 * 2 + 1], -2999.0);
    }

    #[test]
    fn double_step_skips_frames() {
        let mut s = StreamState::new(Box::new(Ramp { n: 100, i: 0 }));
        let mut out = vec![0.0; 20];
        let (n, used) = s.read(&mut out, 2.0);
        assert_eq!(n, 10);
        assert_eq!(used, 20.0);
        for k in 0..10 {
            assert!((out[k * 2] - (k * 2) as f32).abs() < 1e-4);
        }
    }

    #[test]
    fn half_step_interpolates() {
        let mut s = StreamState::new(Box::new(Ramp { n: 100, i: 0 }));
        let mut out = vec![0.0; 8];
        s.read(&mut out, 0.5);
        let left: Vec<f32> = out.chunks(2).map(|f| f[0]).collect();
        assert_eq!(left, vec![0.0, 0.5, 1.0, 1.5]);
    }
}
