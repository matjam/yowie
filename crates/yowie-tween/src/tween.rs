use crate::{Chain, Ease, Lerp};

/// How many extra plays a [`Tween`] makes after the first.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Repeat {
    /// Play this many more times, then finish. `Times(0)` plays once.
    Times(u32),
    /// Play until [`Tween::reset`] or the tween is dropped.
    Forever,
}

/// Animates a value of type `T` from one end to another over a duration.
///
/// Configure with the builder methods, then call [`Tween::update`] each
/// frame. The delay applies once, before the first play. With
/// [`Tween::yoyo`], every second play runs from `to` back to `from`.
#[derive(Clone, Debug)]
pub struct Tween<T> {
    from: T,
    to: T,
    duration: f32,
    delay: f32,
    ease: Ease,
    repeat: Repeat,
    yoyo: bool,
    elapsed: f32,
    plays: u32,
    reversed: bool,
    done: bool,
    over: f32,
}

impl<T: Lerp + Clone> Tween<T> {
    /// Creates a linear tween from `from` to `to` over `seconds`. A
    /// duration of zero or less completes as soon as the delay passes.
    pub fn new(from: T, to: T, seconds: f32) -> Self {
        Self {
            from,
            to,
            duration: seconds,
            delay: 0.0,
            ease: Ease::Linear,
            repeat: Repeat::Times(0),
            yoyo: false,
            elapsed: 0.0,
            plays: 0,
            reversed: false,
            done: false,
            over: 0.0,
        }
    }

    /// Sets the easing.
    pub fn ease(mut self, ease: Ease) -> Self {
        self.ease = ease;
        self
    }

    /// Sets the seconds to wait before the first play.
    pub fn delay(mut self, seconds: f32) -> Self {
        self.delay = seconds.max(0.0);
        self
    }

    /// Sets how many times to play again after the first play.
    pub fn repeat(mut self, repeat: Repeat) -> Self {
        self.repeat = repeat;
        self
    }

    /// Makes repeated plays alternate direction.
    pub fn yoyo(mut self) -> Self {
        self.yoyo = true;
        self
    }

    /// Advances by `dt` seconds and returns the current value.
    pub fn update(&mut self, dt: f32) -> T {
        if self.done {
            return self.value();
        }
        self.elapsed += dt.max(0.0);
        if self.duration <= 0.0 {
            if self.elapsed >= self.delay {
                self.over = self.elapsed - self.delay;
                self.elapsed = self.delay;
                self.done = true;
            }
            return self.value();
        }
        while self.elapsed - self.delay >= self.duration {
            let exhausted = match self.repeat {
                Repeat::Times(n) => self.plays >= n,
                Repeat::Forever => false,
            };
            if exhausted {
                self.over = self.elapsed - self.delay - self.duration;
                self.elapsed = self.delay + self.duration;
                self.done = true;
                break;
            }
            self.plays += 1;
            self.elapsed -= self.duration;
            if self.yoyo {
                self.reversed = !self.reversed;
            }
        }
        self.value()
    }

    /// Eased progress of the current play, from 0 to 1 (Back and Elastic
    /// eases leave that range).
    pub fn progress(&self) -> f32 {
        if self.duration <= 0.0 {
            return 1.0;
        }
        let t = ((self.elapsed - self.delay) / self.duration).clamp(0.0, 1.0);
        self.ease.apply(t)
    }

    /// The current value without advancing.
    pub fn value(&self) -> T {
        let p = self.progress();
        if self.reversed {
            self.to.lerp(&self.from, p)
        } else {
            self.from.lerp(&self.to, p)
        }
    }

    /// Returns true after the last play has finished.
    pub fn done(&self) -> bool {
        self.done
    }

    /// Seconds of the last `update` that came after the tween finished.
    /// [`Chain`] passes this to the next tween.
    pub fn overshoot(&self) -> f32 {
        self.over
    }

    /// Restarts from the beginning, including the delay.
    pub fn reset(&mut self) {
        self.elapsed = 0.0;
        self.plays = 0;
        self.reversed = false;
        self.done = false;
        self.over = 0.0;
    }

    /// Plays `next` after this tween finishes.
    pub fn then(self, next: Tween<T>) -> Chain<T> {
        Chain::new(self).then(next)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(mut tw: Tween<f32>, dts: &[f32]) -> Vec<f32> {
        dts.iter().map(|&dt| tw.update(dt)).collect()
    }

    #[test]
    fn linear_progress_and_done() {
        let mut tw = Tween::new(0.0_f32, 10.0, 2.0);
        assert_eq!(tw.update(0.5), 2.5);
        assert!(!tw.done());
        assert_eq!(tw.update(5.0), 10.0);
        assert!(tw.done());
        assert!((tw.overshoot() - 3.5).abs() < 1e-6);
        assert_eq!(tw.update(1.0), 10.0);
        tw.reset();
        assert_eq!(tw.value(), 0.0);
    }

    #[test]
    fn delay_holds_the_start_value() {
        let got = run(Tween::new(0.0, 4.0, 2.0).delay(1.0), &[0.5, 0.5, 1.0, 5.0]);
        assert_eq!(got, [0.0, 0.0, 2.0, 4.0]);
    }

    #[test]
    fn repeat_restarts_each_play() {
        let tw = Tween::new(0.0, 1.0, 1.0).repeat(Repeat::Times(1));
        assert_eq!(run(tw, &[0.5, 0.75, 0.25, 10.0]), [0.5, 0.25, 0.5, 1.0]);
    }

    #[test]
    fn yoyo_goes_back_and_forth() {
        let tw = Tween::new(0.0, 1.0, 1.0).repeat(Repeat::Times(2)).yoyo();
        let got = run(tw, &[0.5, 1.0, 1.0, 1.0]);
        assert_eq!(got, [0.5, 0.5, 0.5, 1.0]);
        let mut tw = Tween::new(0.0, 1.0, 1.0).repeat(Repeat::Times(1)).yoyo();
        tw.update(3.0);
        assert!(tw.done());
        assert_eq!(tw.value(), 0.0);
    }

    #[test]
    fn forever_never_finishes() {
        let mut tw = Tween::new(0.0, 1.0, 1.0).repeat(Repeat::Forever);
        for _ in 0..100 {
            tw.update(0.7);
        }
        assert!(!tw.done());
    }

    #[test]
    fn zero_duration_completes_after_delay() {
        let mut tw = Tween::new(1.0, 5.0, 0.0).delay(1.0);
        assert_eq!(tw.update(0.5), 5.0);
        assert!(!tw.done());
        assert_eq!(tw.update(0.5), 5.0);
        assert!(tw.done());
    }

    #[test]
    fn ease_shapes_the_value() {
        let tw = Tween::new(0.0, 1.0, 1.0).ease(Ease::In(crate::Curve::Quad));
        assert_eq!(run(tw, &[0.5]), [0.25]);
    }
}
