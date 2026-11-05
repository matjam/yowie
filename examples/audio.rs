//! The sound tour: three positional voices orbiting the listener with
//! Doppler and occlusion on keys, stereo panning or the binaural head model,
//! a shared reverb and a low-pass filter on keys, fades, pitch, voice
//! priorities under a small voice cap, and a synthesised music stream.
//!
//! - `--binaural` starts with the binaural head model (B toggles it).
//! - `--zone` puts a reverb hall over the left half of the window and moves
//!   the listener with the mouse, so the room changes as sources pass
//!   through it.
//! - `--music <file>` streams an Ogg, MP3, FLAC or WAV file instead of the
//!   synthesised arpeggio.
//! - `--mic` records from the default microphone and draws a level meter.
//!
//! Headless runs (`--shot`) use the engine's offline mixer and never open a
//! device, so they make no sound.

#[path = "common/mod.rs"]
mod common;

use yowie::audio::{
    Capture, Music, MusicOptions, PlayOptions, Positional, ReverbSettings, ReverbZone, Sound,
    SpatialSettings, Stream, Voice,
};
use yowie::gfx::Font;
use yowie::input::Key;
use yowie::{Color, Config, Context, Game, GameResult, Rect, Vec3, vec2};

/// A source that circles the listener.
struct Source {
    voice: Voice,
    color: Color,
    speed: f32,
    dist: f32,
    pos: Vec3,
}

struct Tour {
    font: Font,
    zone: bool,
    tone: Sound,
    click: Sound,
    pad: Voice,
    stream: Voice,
    music: Option<(String, yowie::audio::MusicControl)>,
    sources: Vec<Source>,
    capture: Option<Capture>,
    mic_note: String,
    mic_buf: Vec<f32>,
    mic_peak: f32,
    bursts: Vec<Voice>,
    reverb: f32,
    room: f32,
    cutoff: f32,
    pitch: f32,
    occlusion: f32,
    doppler: f32,
    binaural: bool,
    pad_level: f32,
    paused: bool,
}

/// A slow chord arpeggio synthesised on demand: how procedural music or a
/// custom decoder plugs into the mixer.
struct Arpeggio {
    rate: f64,
    t: f64,
    phase: f64,
}

const NOTES: [f64; 6] = [261.63, 329.63, 392.0, 523.25, 392.0, 329.63];

impl Stream for Arpeggio {
    fn read(&mut self, out: &mut [f32]) -> usize {
        let frames = out.len() / 2;
        for frame in out.as_chunks_mut::<2>().0 {
            let step = (self.t * 3.0) as usize % NOTES.len();
            let env = 1.0 - (self.t * 3.0).fract();
            self.phase += std::f64::consts::TAU * NOTES[step] / self.rate;
            let s = ((self.phase.sin() * 0.6 + (2.0 * self.phase).sin() * 0.25) * env) as f32;
            frame[0] = s;
            frame[1] = s;
            self.t += 1.0 / self.rate;
        }
        frames
    }
}

fn flag(name: &str) -> bool {
    std::env::args().any(|a| a == name)
}

fn value(name: &str) -> Option<String> {
    let mut args = std::env::args();
    while let Some(a) = args.next() {
        if a == name {
            return args.next();
        }
    }
    None
}

/// Steps a value with a pair of keys: the first lowers it, the second raises it.
fn nudge(ctx: &Context, down: Key, up: Key, v: &mut f32, step: f32, lo: f32, hi: f32) -> bool {
    let before = *v;
    for (key, dir) in [(down, -1.0), (up, 1.0)] {
        if ctx.input.key_pressed(key) || ctx.input.key_repeated(key) {
            *v = (*v + dir * step).clamp(lo, hi);
        }
    }
    *v != before
}

impl Game for Tour {
    fn init(ctx: &mut Context) -> GameResult<Self> {
        let m = ctx.audio.clone();
        m.set_max_voices(12); // small on purpose, so the burst shows stealing
        let rate = m.rate();
        let tone = m.sound(&yowie::audio::Pcm::sine(440.0, 0.3, rate))?;
        let click = m.sound(&yowie::audio::Pcm::sine(1200.0, 0.05, rate))?;
        let pad_sound = m.sound(&yowie::audio::Pcm::sine(110.0, 2.0, rate))?;
        let (reverb, room, cutoff) = (0.3, 0.6, 4000.0);
        m.set_reverb(ReverbSettings {
            room_size: room,
            wet: reverb,
            ..Default::default()
        });
        // Doppler in pixels: a high speed of sound keeps the shift to about
        // a semitone at full factor.
        m.set_doppler(1.0);
        m.set_speed_of_sound(3000.0);
        let binaural = flag("--binaural");
        m.set_spatial(SpatialSettings {
            binaural,
            ..Default::default()
        });
        let pad = m.play(
            &pad_sound,
            PlayOptions {
                looping: true,
                volume: 0.35,
                low_pass: Some(cutoff),
                reverb: 1.0,
                fade_in: 1.5,
                priority: 10,
                ..Default::default()
            },
        );
        let mut sources = Vec::new();
        for (i, f) in [330.0, 495.0, 660.0].into_iter().enumerate() {
            let snd = m.sound(&yowie::audio::Pcm::sine(f, 1.0, rate))?;
            let voice = m.play(
                &snd,
                PlayOptions {
                    looping: true,
                    volume: 0.3,
                    priority: 10,
                    positional: Some(Positional {
                        min_distance: 60.0,
                        max_distance: 500.0,
                        ..Positional::at(Vec3::ZERO)
                    }),
                    ..Default::default()
                },
            );
            let k = i as u8;
            sources.push(Source {
                voice,
                color: Color::rgb8(255 - 80 * k, 120 + 60 * k, 80 * k + 60),
                speed: 0.4 + 0.3 * i as f32,
                dist: 120.0 + 90.0 * i as f32,
                pos: Vec3::ZERO,
            });
        }
        // Music: a file if given, otherwise a synthesised arpeggio stream.
        let (stream, music) = match value("--music") {
            Some(path) => {
                let music = Music::open_file(
                    &m,
                    &path,
                    MusicOptions {
                        looping: true,
                        ..Default::default()
                    },
                )?;
                let control = music.control();
                let voice = m.play_stream(
                    music,
                    PlayOptions {
                        volume: 0.5,
                        priority: 20,
                        ..Default::default()
                    },
                );
                (voice, Some((path, control)))
            }
            None => {
                let voice = m.play_stream(
                    Arpeggio {
                        rate: f64::from(rate),
                        t: 0.0,
                        phase: 0.0,
                    },
                    PlayOptions {
                        volume: 0.18,
                        reverb: 0.6,
                        priority: 20,
                        ..Default::default()
                    },
                );
                (voice, None)
            }
        };
        // The microphone opens only on request and never in a headless run.
        let (mut capture, mut mic_note, mut mic_buf) = (None, String::new(), Vec::new());
        if flag("--mic") && !ctx.headless() {
            match Capture::open_default() {
                Ok(c) => {
                    mic_buf = vec![0.0; (c.rate() / 10) as usize * usize::from(c.channels())];
                    capture = Some(c);
                }
                Err(e) => mic_note = e.to_string(),
            }
        }
        Ok(Tour {
            font: ctx.gfx.default_font(),
            zone: flag("--zone"),
            tone,
            click,
            pad,
            stream,
            music,
            sources,
            capture,
            mic_note,
            mic_buf,
            mic_peak: 0.0,
            bursts: Vec::new(),
            reverb,
            room,
            cutoff,
            pitch: 1.0,
            occlusion: 0.0,
            doppler: 1.0,
            binaural,
            pad_level: 0.35,
            paused: false,
        })
    }

    fn update(&mut self, ctx: &mut Context) -> GameResult {
        if ctx.input.key_pressed(Key::Escape) {
            ctx.quit();
        }
        let m = ctx.audio.clone();
        if nudge(
            ctx,
            Key::Digit1,
            Key::Digit2,
            &mut self.reverb,
            0.05,
            0.0,
            1.0,
        ) | nudge(
            ctx,
            Key::Digit3,
            Key::Digit4,
            &mut self.room,
            0.05,
            0.05,
            1.0,
        ) {
            m.set_reverb(ReverbSettings {
                room_size: self.room,
                wet: self.reverb,
                ..Default::default()
            });
        }
        if nudge(
            ctx,
            Key::Digit5,
            Key::Digit6,
            &mut self.cutoff,
            200.0,
            200.0,
            8000.0,
        ) {
            self.pad.set_low_pass(Some(self.cutoff));
        }
        if nudge(
            ctx,
            Key::Digit7,
            Key::Digit8,
            &mut self.occlusion,
            0.1,
            0.0,
            1.0,
        ) {
            for s in &self.sources {
                s.voice.set_occlusion(self.occlusion);
            }
        }
        if nudge(
            ctx,
            Key::Digit9,
            Key::Digit0,
            &mut self.doppler,
            0.25,
            0.0,
            3.0,
        ) {
            m.set_doppler(self.doppler);
        }
        nudge(ctx, Key::Q, Key::E, &mut self.pitch, 0.1, 0.5, 2.0);
        if ctx.input.key_pressed(Key::B) {
            self.binaural = !self.binaural;
            m.set_spatial(SpatialSettings {
                binaural: self.binaural,
                ..Default::default()
            });
        }
        if ctx.input.key_pressed(Key::C) {
            m.play(
                &self.click,
                PlayOptions {
                    volume: 0.6,
                    pitch: self.pitch,
                    reverb: self.reverb,
                    ..Default::default()
                },
            );
        }
        if ctx.input.key_pressed(Key::X) {
            // Low-priority voices; the cap makes the mixer steal the quietest.
            self.bursts.clear();
            for i in 0..40 {
                self.bursts.push(m.play(
                    &self.tone,
                    PlayOptions {
                        volume: 0.2,
                        pitch: 0.8 + 0.02 * i as f32,
                        pan: ((i % 5) as f32 - 2.0) / 2.0,
                        ..Default::default()
                    },
                ));
            }
        }
        if ctx.input.key_pressed(Key::O) {
            self.pad_level = 0.0;
            self.pad.fade_to(0.0, 1.5);
        }
        if ctx.input.key_pressed(Key::I) {
            self.pad_level = 0.35;
            self.pad.fade_to(0.35, 1.5);
        }
        if ctx.input.key_pressed(Key::Space) {
            self.paused = !self.paused;
            self.stream.set_paused(self.paused);
        }

        // The listener sits at the window centre; sources circle it. With
        // --zone a hall covers the left half and the mouse moves the listener.
        let size = ctx.window_size();
        let (cx, cy) = (size.x as f32 / 2.0, size.y as f32 / 2.0);
        let (mut lx, mut ly) = (cx, cy);
        if self.zone {
            let mouse = ctx.input.mouse();
            (lx, ly) = (mouse.x, mouse.y);
            m.set_reverb_zones(&[ReverbZone {
                center: Vec3::new(cx / 2.0, cy, 0.0),
                radius: cx / 2.0,
                fade: cx / 8.0,
                settings: ReverbSettings {
                    room_size: 0.95,
                    damping: 0.2,
                    wet: 0.8,
                    ..Default::default()
                },
            }]);
        }
        m.set_listener(yowie::audio::Listener::new_2d(lx, ly));
        let t = ctx.time.elapsed() as f32;
        for s in &mut self.sources {
            let a = t * s.speed;
            s.pos = Vec3::new(cx + s.dist * a.cos(), cy + s.dist * a.sin(), 0.0);
            s.voice.set_position(s.pos);
            // The orbit's tangent in pixels per second drives Doppler.
            let v = s.dist * s.speed;
            s.voice
                .set_velocity(Vec3::new(-v * a.sin(), v * a.cos(), 0.0));
        }
        if let Some(c) = &mut self.capture {
            // Drain what the device recorded so its ring never fills, and
            // hold the peak a moment so the meter is readable.
            while c.read(&mut self.mic_buf) > 0 {}
            self.mic_peak = c.level().peak.max(self.mic_peak - ctx.time.delta());
        }
        Ok(())
    }

    fn draw(&mut self, ctx: &mut Context) -> GameResult {
        let size = ctx.window_size();
        let (cx, cy) = (size.x as f32 / 2.0, size.y as f32 / 2.0);
        for s in &self.sources {
            for r in [9.0, 18.0, 27.0] {
                ctx.gfx
                    .fill_circle(vec2(s.pos.x, s.pos.y), r, s.color.with_alpha(0.2));
            }
            ctx.gfx.fill_circle(vec2(s.pos.x, s.pos.y), 6.0, s.color);
        }
        let l = if self.zone {
            ctx.input.mouse()
        } else {
            vec2(cx, cy)
        };
        if self.zone {
            ctx.gfx.fill_rect(
                Rect::new(0.0, 0.0, cx, size.y as f32),
                Color::rgba8(80, 90, 160, 40),
            );
            common::label(
                ctx,
                &self.font,
                "hall: move the mouse to walk the listener in and out",
                vec2(16.0, size.y as f32 - 30.0),
                14.0,
                common::TEXT,
            );
        }
        ctx.gfx.fill_circle(l, 8.0, Color::rgb8(240, 240, 250));
        common::label(
            ctx,
            &self.font,
            "listener",
            l + vec2(-24.0, 12.0),
            14.0,
            common::DIM,
        );

        let playing = self.sources.iter().filter(|s| s.voice.is_playing()).count()
            + usize::from(self.pad.is_playing())
            + usize::from(self.stream.is_playing())
            + self.bursts.iter().filter(|v| v.is_playing()).count();
        let mut lines = vec![
            format!("{playing} voices playing (cap 12)"),
            format!(
                "1/2 reverb wet {:.2}   3/4 room {:.2}",
                self.reverb, self.room
            ),
            format!("5/6 pad low-pass {:.0} Hz", self.cutoff),
            format!(
                "7/8 orbit occlusion {:.1}   9/0 doppler {:.2}",
                self.occlusion, self.doppler
            ),
            format!(
                "B binaural: {}   Q/E click pitch {:.1}",
                if self.binaural {
                    "on"
                } else {
                    "off (stereo pan)"
                },
                self.pitch
            ),
            "C click   X burst of 40 (watch the cap)".into(),
            format!("O/I fade pad out/in (level {:.2})", self.pad_level),
            format!(
                "Space {} the music stream",
                if self.paused { "resumes" } else { "pauses" }
            ),
        ];
        match &self.music {
            Some((path, control)) => lines.push(match control.error() {
                Some(e) => format!("{path}: {e}"),
                None => format!("Streaming {path}"),
            }),
            None => {
                lines.push("Music is a synthesised Stream; --music file.ogg streams a file".into())
            }
        }
        let height =
            20.0 + lines.len() as f32 * 20.0 + if self.capture.is_some() { 30.0 } else { 0.0 };
        common::panel(ctx, Rect::new(16.0, 16.0, 420.0, height));
        for (i, line) in lines.iter().enumerate() {
            common::label(
                ctx,
                &self.font,
                line,
                vec2(28.0, 24.0 + i as f32 * 20.0),
                14.0,
                common::TEXT,
            );
        }
        if let Some(c) = &self.capture {
            // A decibel meter with silence at -60 dB.
            let db = if self.mic_peak > 0.0 {
                ((20.0 * self.mic_peak.log10() + 60.0) / 60.0).max(0.0)
            } else {
                0.0
            };
            let y = 24.0 + lines.len() as f32 * 20.0;
            common::bar(ctx, Rect::new(28.0, y, 380.0, 12.0), db, common::ACCENT);
            common::label(
                ctx,
                &self.font,
                &format!("Mic at {} Hz, {} dropped", c.rate(), c.dropped_samples()),
                vec2(28.0, y + 14.0),
                12.0,
                common::DIM,
            );
        } else if !self.mic_note.is_empty() {
            common::label(
                ctx,
                &self.font,
                &format!("Microphone: {}", self.mic_note),
                vec2(28.0, height - 4.0),
                12.0,
                common::DIM,
            );
        }
        Ok(())
    }
}

fn main() -> Result<(), yowie::Error> {
    yowie::run::<Tour>(Config::new("Yowie audio").with_size(900, 600))
}
