---
title: Audio
group: Audio
order: 1
summary: sounds, streamed music, voices and buses, reverb and filters, spatial voices, capture, tracker music and offline mixing
---

# Audio

To play sound, use `ctx.audio`. It is a `yowie::audio::Mixer`, which turns
decoded or generated audio into sounds, plays them as voices, groups voices
on buses, places them in the world and streams music. The mixer comes from
the `yowie-audio` crate, which the root crate re-exports as `yowie::audio`
when the `audio` cargo feature is on. The feature is on by default.

This guide covers the tasks in the order a game meets them. For every type
and method, run `cargo doc --open` and read the `yowie::audio` module.

To hear most of what follows, run the audio tour:

```bash
cargo run --example audio
```

| Flag | Effect |
|---|---|
| `--binaural` | starts with the binaural head model; B toggles it |
| `--zone` | puts a reverb zone over the left half of the window, and the mouse moves the listener |
| `--music <file>` | streams an Ogg Vorbis, MP3, FLAC or WAV file instead of the synthesised arpeggio |
| `--mic` | records from the default microphone and draws a level meter |

The window lists the keys that change reverb, filters, occlusion, Doppler,
pitch and fades while it runs.

## The mixer in a game

The engine makes the mixer before `Game::init` and stores it in
`Context::audio`. `Mixer` is a cheap handle. Clone it into your game state
or into an ECS system; every clone controls the same mixer, and any thread
may call it.

Windowed runs play the mixer on the system's default output device. These
runs get an offline mixer instead, which the engine renders every frame and
discards:

- headless runs, such as `--shot`;
- runs whose `Config` has `audio` set to false (`Config::with_audio(false)`);
- runs where no output device opens. The engine logs a warning and carries on.

With the offline mixer, voices start, advance with game time, finish and
call back as they would with a device, so game code needs no separate path.
[Offline mixing](#offline-mixing-and-headless-runs) has the details.

The engine calls `Mixer::update` once per frame, after the frame's updates.
It runs end-of-voice callbacks and frees voices the audio thread has
finished with.

On Linux, the ALSA backend needs the ALSA development files:
`libasound2-dev` on Debian and Ubuntu, `alsa-lib-devel` on Fedora. macOS and
Windows need nothing extra.

A game that plays a sound when the player jumps:

```rust,no_run
use yowie::audio::{Mixer, PlayOptions, Sound};
use yowie::input::Key;
use yowie::{Config, Context, Game, GameResult};

struct Jumper {
    audio: Mixer,
    jump: Sound,
}

impl Game for Jumper {
    fn init(ctx: &mut Context) -> GameResult<Self> {
        let jump = ctx.load_sound("sounds/jump.ogg")?;
        Ok(Jumper {
            audio: ctx.audio.clone(),
            jump,
        })
    }

    fn update(&mut self, ctx: &mut Context) -> GameResult {
        if ctx.input.key_pressed(Key::Space) {
            self.audio.play(&self.jump, PlayOptions::default());
        }
        Ok(())
    }

    fn draw(&mut self, _ctx: &mut Context) -> GameResult {
        Ok(())
    }
}

fn main() -> Result<(), yowie::Error> {
    yowie::run::<Jumper>(Config::new("Jumper"))
}
```

## Loading sounds

To load a sound from the game's assets, call `Context::load_sound` with its
asset path. It reads WAV, Ogg Vorbis, MP3 and FLAC from the asset
directories and pack files, detects the format from the data, decodes the
whole file and converts it to the mixer's sample rate. The
[game services guide](services.md) explains where assets are read from.

The result is a `Sound`, which holds the decoded samples in memory. Cloning
a `Sound` is cheap, because clones share the samples. A sound is stored as
stereo 32-bit floats at the mixer's rate, mono sources included, so every
second costs 384 KB at 48 kHz. Use sounds for effects and short clips, and
stream long music with [`Music`](#streaming-music).

To decode outside the asset system, use the `audio::decode` module:

| Function | Reads |
|---|---|
| `decode::decode(bytes)` | a whole file held in a `Vec<u8>` |
| `decode::decode_file(path)` | a file on disk; the extension is a hint to the detector |

Both return a `Pcm`, which holds interleaved `f32` samples, a channel count
and a rate. Sources with more than two channels are mixed down to stereo. To
turn a `Pcm` into a `Sound`, call `Mixer::sound`. It resamples by linear
interpolation to the mixer's rate, and fails unless the PCM has one or two
channels, a nonzero rate and a whole number of frames.

```rust,no_run
use yowie::audio::{Mixer, Sound, decode};

fn load_voice_line(mixer: &Mixer, path: &str) -> yowie::audio::Result<Sound> {
    let pcm = decode::decode_file(path)?;
    mixer.sound(&pcm)
}
```

A `Sound` holds samples at the rate of the mixer that made it, and
`Mixer::play` does not convert them. Make each sound with the mixer that
plays it. `ctx.load_sound` uses the rate of `ctx.audio`.

`Pcm` and `Sound` report their length with `frames()` and `duration()`, in
seconds.

## Generating sounds

To make a placeholder tone, call `Pcm::sine(freq, seconds, rate)`. It
returns a mono sine wave at `freq` hertz, with 10 ms fades at both ends. The
Tetris example builds all its sounds this way.

To synthesise game effects, use `audio::synth::Tone`, one oscillator shaped
by an attack, decay, sustain, release envelope. `Tone::render(rate)` returns
mono PCM that includes the release.

| `Tone` field | Default | Meaning |
|---|---|---|
| `waveform` | `Waveform::Sine` | `Sine`, `Square`, `Saw`, `Triangle` or `Noise` |
| `frequency` | 440.0 | pitch in Hz at the start of the note |
| `sweep_to` | `None` | pitch in Hz at the end of the held note; the pitch glides linearly |
| `duration` | 0.25 | seconds the note is held before its release |
| `volume` | 0.5 | peak amplitude, 0 to 1 |
| `adsr` | `Adsr::default()` | attack 0.005 s, decay 0.05 s, sustain level 0.7, release 0.1 s |

`Square` and `Saw` are band-limited. `Noise` uses a fixed seed, so a noise
tone renders the same samples every time.

```rust
use yowie::audio::synth::{Adsr, Tone, Waveform};
use yowie::audio::{Mixer, Sound};

fn laser(mixer: &Mixer) -> yowie::audio::Result<Sound> {
    let tone = Tone {
        waveform: Waveform::Square,
        frequency: 1200.0,
        sweep_to: Some(200.0),
        duration: 0.15,
        volume: 0.4,
        adsr: Adsr {
            attack: 0.0,
            decay: 0.05,
            sustain: 0.6,
            release: 0.05,
        },
    };
    mixer.sound(&tone.render(mixer.rate()))
}

fn main() -> yowie::audio::Result<()> {
    let (mixer, _renderer) = Mixer::offline(48_000)?;
    let zap = laser(&mixer)?;
    // 0.15 s held plus a 0.05 s release.
    assert!((zap.duration() - 0.2).abs() < 0.01);
    Ok(())
}
```

## Playing a sound

To play a sound, call `Mixer::play` with the sound and a `PlayOptions`. It
returns a `Voice`, a handle to that one playback. `PlayOptions::default()`
plays once at unity gain, centred, through the master bus.

| `PlayOptions` field | Default | Meaning |
|---|---|---|
| `volume` | 1.0 | gain; 1 is unity |
| `pan` | 0.0 | stereo position from -1 (left) to +1 (right); ignored for positional voices |
| `pitch` | 1.0 | playback rate; 2 plays an octave up. Clamped to 0.01 to 64; zero or a non-finite value means 1 |
| `looping` | false | repeat the sound; a stream controls its own looping |
| `loop_points` | `None` | loop region `(start, end)` in seconds; `None` loops the whole sound |
| `fade_in` | 0.0 | seconds to rise from silence to `volume` |
| `priority` | 0 | a higher priority keeps its slot when the mixer is full |
| `bus` | `None` | the [bus](#buses) to play through; `None` is the master bus |
| `reverb` | 0.0 | send level, 0 to 1, into the bus's reverb or the shared [reverb](#reverb) |
| `low_pass` | `None` | low-pass cutoff in Hz |
| `occlusion` | 0.0 | 0 clear to 1 fully blocked; see [Occlusion](#occlusion) |
| `positional` | `None` | places the voice in the world; see [Spatial audio](#spatial-audio) |

Panning keeps constant power, so a centred voice plays at about 0.707 of its
gain (3 dB down) in each channel.

Dropping a `Voice` does not stop it, so you can ignore the handle of a sound
that plays once. Keep the handle to control the voice or to ask about it:

| Method | Returns |
|---|---|
| `is_playing()` | true while the voice is active, including while paused, muted or out of range; false as soon as it is stopped or plays out |
| `is_finished()` | true once the voice has ended and left the mixer, including its stop ramp |
| `position()` | seconds into the sound, as of the last chunk mixed |

The mixer runs without a device in a test or a plain `main`, so this block
plays a sound and checks that it ends:

```rust
use yowie::audio::{Mixer, Pcm, PlayOptions};

fn main() -> yowie::audio::Result<()> {
    let (mixer, mut renderer) = Mixer::offline(48_000)?;
    let beep = mixer.sound(&Pcm::sine(880.0, 0.1, 48_000))?;
    let voice = mixer.play(
        &beep,
        PlayOptions {
            volume: 0.5,
            pan: -0.5,
            pitch: 1.5,
            ..Default::default()
        },
    );
    assert!(voice.is_playing());
    // A tenth of a second at pitch 1.5 lasts about 67 ms.
    renderer.render_frames(4_800);
    assert!(voice.is_finished());
    Ok(())
}
```

## Controlling a voice

To change a playing voice, call its setters. Each one queues a change that
the mixer applies at the start of its next chunk of 256 frames (about 5 ms at
48 kHz). Gain changes ramp across that chunk, so they do not click. Setters
on a finished voice do nothing.

| Method | Effect |
|---|---|
| `set_volume(v)` | sets the gain; 1 is unity. Cancels a fade in progress |
| `fade_to(v, seconds)` | moves the gain to `v` over `seconds` |
| `fade_out(seconds)` | fades to silence over `seconds`, then stops the voice |
| `set_pan(p)` | stereo position from -1 to +1 |
| `set_pitch(p)` | playback rate, clamped to 0.01 to 64 |
| `set_paused(bool)` | holds the voice in place, silent, until resumed |
| `set_mute(bool)` | silences the voice while it keeps advancing |
| `set_solo(bool)` | while any voice is soloed, voices that are not soloed are silent |
| `stop()` | ramps to silence over about a millisecond, then ends the voice; `is_playing` turns false at once |
| `seek(seconds)` | moves a sound to `seconds` from its start, or asks a stream to seek |
| `set_looping(bool)` | turns looping of a sound on or off |
| `set_low_pass(Some(hz))` | sets a low-pass cutoff in Hz; `None` removes the filter |
| `set_reverb(send)` | sets the reverb send level |
| `set_occlusion(o)` | sets how blocked the source is, 0 to 1 |

The spatial setters are listed under [Positional voices](#positional-voices).

To pause a voice and keep its place, use `set_paused`. To keep it running in
time with the game while it cannot be heard, use `set_mute`.

```rust
use yowie::audio::Voice;

fn on_engine_damaged(engine_hum: &Voice) {
    engine_hum.set_pitch(0.8);
    engine_hum.set_low_pass(Some(900.0));
}

fn on_level_end(music: &Voice) {
    music.fade_out(2.0);
}
```

### When a voice ends

To run code when a voice ends, call `Voice::on_done` with a closure. The
closure runs once, whether the voice played out, was stopped or lost its
slot. It runs on the thread that calls `Mixer::update`, which in a game is
the main thread after the frame's updates. It must be `Send + 'static`, so
capture clones of the mixer and the sounds it needs:

```rust
use yowie::audio::{Mixer, PlayOptions, Sound};

fn play_then_follow(mixer: &Mixer, first: &Sound, second: &Sound) {
    let next = mixer.clone();
    let second = second.clone();
    mixer
        .play(first, PlayOptions::default())
        .on_done(move || {
            next.play(&second, PlayOptions::default());
        });
}
```

The engine runs callbacks at the end of a frame, so the second voice starts
up to a frame after the first ends, with silence between them. To play an
intro and then a loop with no gap, put both parts in one sound and use loop
points.

### Loop points

To loop part of a sound, set `looping` and `loop_points`. The voice plays
from the start to the loop end, then repeats the region from the loop start.
To let a looping voice play on to the end of the sound, call
`set_looping(false)`.

```rust
use yowie::audio::{Mixer, PlayOptions, Sound, Voice};

/// Plays a 2 s intro once, then repeats the section from 2 s to 10 s.
fn start_theme(mixer: &Mixer, theme: &Sound) -> Voice {
    mixer.play(
        theme,
        PlayOptions {
            looping: true,
            loop_points: Some((2.0, 10.0)),
            bus: Some(mixer.music()),
            ..Default::default()
        },
    )
}
```

## Buses

To group voices, play them on a bus. A mixer starts with three buses:

| Bus | Name | Use |
|---|---|---|
| `Mixer::master()` | `master` | every other bus feeds it; `PlayOptions::bus` of `None` plays here |
| `Mixer::music()` | `music` | music |
| `Mixer::effects()` | `effects` | sound effects |

To make another, call `Mixer::new_bus(name)`. It returns the existing bus
when the name is taken, and fails with `Error::TooManyBuses` past
`MAX_BUSES`, which is 32 including the built-in three. `Mixer::bus(name)`
finds a bus by name. Every bus feeds the master bus directly; buses do not
nest.

A `Bus` is a handle, and clones refer to the same bus. Its settings take
effect on the next chunk and ramp, so changing them does not click.

| Method | Effect |
|---|---|
| `set_volume(v)` / `volume()` | gain on everything on the bus; 1 is unity, negative values become 0 |
| `set_mute(bool)` / `muted()` | silences the bus while its voices keep playing |
| `set_solo(bool)` / `soloed()` | while any bus is soloed, buses that are not soloed are silent |
| `set_paused(bool)` / `paused()` | holds every voice on the bus in place, silent |
| `set_low_pass(hz)` | low-pass cutoff in Hz over the whole bus; zero or less removes it |
| `set_reverb(settings)` | gives the bus a reverb of its own; see [Bus reverb](#bus-reverb) |
| `name()` | the bus name |

`Mixer::set_master_volume(v)` sets the master bus volume.

A settings screen binds its sliders to buses rather than to voices:

```rust
use yowie::audio::{Mixer, PlayOptions, Sound};

struct VolumeSettings {
    master: f32,
    music: f32,
    effects: f32,
}

fn apply_volume(mixer: &Mixer, settings: &VolumeSettings) {
    mixer.set_master_volume(settings.master);
    mixer.music().set_volume(settings.music);
    mixer.effects().set_volume(settings.effects);
}

fn play_footstep(mixer: &Mixer, step: &Sound) -> yowie::audio::Result<()> {
    let footsteps = mixer.new_bus("footsteps")?;
    mixer.play(
        step,
        PlayOptions {
            bus: Some(footsteps),
            volume: 0.6,
            ..Default::default()
        },
    );
    Ok(())
}
```

Bus solo applies to the buses that feed the master bus. Voices that play on
the master bus itself stay audible while another bus is soloed. To audition
one bus against the rest, play every voice on a named bus.

To see and change the three built-in buses while the game runs, attach the
mixer to the debug console with `ctx.console.attach_mixer(&ctx.audio)`. Its
Audio panel has volume sliders and mute, solo and pause switches. The
[console guide](console.md) covers the console.

## Pausing, muting and stopping

To pause every voice, as a pause menu does, call `Mixer::set_paused(true)`.
`Bus::set_paused` pauses one bus and `Voice::set_paused` pauses one voice.
The three flags are kept separately, and a voice plays only while none of
them holds it, so resuming the mixer leaves a paused bus paused. A pause
fades out over the chunk it lands in, so it does not click.

To silence sound without holding its place, mute it with `Voice::set_mute`
or `Bus::set_mute`. Muted voices advance and finish as usual.

To stop every voice, call `Mixer::stop_all`. Each voice ramps to silence
first.

`Config::pause_when_unfocused` stops game updates only; sound keeps playing.
To pause sound when the window loses focus, handle `Event::Focused`:

```rust
use yowie::{Context, Event, Game, GameResult};

struct Quiet;

impl Game for Quiet {
    fn init(_ctx: &mut Context) -> GameResult<Self> {
        Ok(Quiet)
    }

    fn draw(&mut self, _ctx: &mut Context) -> GameResult {
        Ok(())
    }

    fn event(&mut self, ctx: &mut Context, event: &Event) -> GameResult {
        if let Event::Focused(focused) = event {
            ctx.audio.set_paused(!focused);
        }
        Ok(())
    }
}
```

## Voice limits and priority

To cap how many voices play at once, call `Mixer::set_max_voices(n)`. The
default is 64 and the most is 128. When the mixer is full, a new voice
replaces the quietest voice of the lowest priority that is no higher than
its own, and the replaced voice ramps out. When every playing voice has a
higher priority, the new voice is refused. `play` then returns a voice that
is already finished or finishes at the next chunk, and its `on_done` callback
runs on a following `Mixer::update`.

Give music, dialogue and other sounds that must not drop out a higher
`priority` than bullets and debris:

```rust
use yowie::audio::{Mixer, PlayOptions, Sound, Voice};

fn play_dialogue(mixer: &Mixer, line: &Sound) -> Voice {
    mixer.play(
        line,
        PlayOptions {
            priority: 100,
            ..Default::default()
        },
    )
}
```

## Streaming music

To play a long file without decoding all of it into memory, open it as
`Music` and play it with `Mixer::play_stream`:

| Constructor | Opens |
|---|---|
| `Music::open_file(&mixer, path, options)` | a WAV, Ogg Vorbis, MP3 or FLAC file on disk |
| `Music::open_bytes(&mixer, bytes, options)` | a file already held in a `Vec<u8>` |

A background thread named `yowie-music` decodes the file, resamples it to
the mixer's rate and queues 16 blocks of 2048 frames ahead of playback
(about 0.7 s at 48 kHz). If the decoder falls behind, the voice plays
silence and carries on.

| `MusicOptions` field | Default | Meaning |
|---|---|---|
| `looping` | false | restart at the loop start when the end is reached |
| `loop_range` | `None` | loop region `(start, end)` in seconds from the start of the file; `None` loops the whole file |

`play_stream` moves the music to the audio thread. Before you hand it over,
keep a `MusicControl` from `Music::control()` to control it afterwards:

| `MusicControl` method | Effect |
|---|---|
| `seek(seconds)` | moves playback; queued audio is discarded, so the new position is heard after a short delay |
| `set_looping(bool)` / `looping()` | turns looping on or off from the decoder's current position |
| `set_loop_range(range)` | sets the loop region; fails unless `0 <= start < end` |
| `duration()` | the file's length in seconds, when the container records it |
| `error()` | the decoding error that stopped the music, if any |

`PlayOptions::looping` does not apply to a stream; music loops through
`MusicOptions`. A decoding error ends the voice, and `MusicControl::error`
says why.

```rust,no_run
use yowie::audio::{Music, MusicControl, MusicOptions, PlayOptions, Voice};
use yowie::{Context, GameResult};

fn start_music(ctx: &Context, path: &str) -> GameResult<(Voice, MusicControl)> {
    let options = MusicOptions {
        looping: true,
        ..Default::default()
    };
    let music = Music::open_file(&ctx.audio, path, options)?;
    let control = music.control();
    let voice = ctx.audio.play_stream(
        music,
        PlayOptions {
            bus: Some(ctx.audio.music()),
            fade_in: 1.0,
            priority: 100,
            ..Default::default()
        },
    );
    Ok((voice, control))
}
```

`Music::open_file` reads the disk. To stream music from the asset
directories or a pack file, read its bytes through the asset file system and
open them with `Music::open_bytes`. The compressed file stays in memory and
is decoded as it plays.

```rust,no_run
use yowie::audio::{Music, MusicOptions};
use yowie::{Context, GameResult};

fn open_asset_music(ctx: &Context, path: &str) -> GameResult<Music> {
    let bytes = ctx.assets.vfs().read(path)?;
    let options = MusicOptions {
        looping: true,
        ..Default::default()
    };
    Ok(Music::open_bytes(&ctx.audio, bytes, options)?)
}
```

To crossfade from one track to the next, fade the old voice out and start
the new one with `fade_in`:

```rust
use yowie::audio::{Mixer, Music, PlayOptions, Voice};

fn crossfade(mixer: &Mixer, old: &Voice, next: Music, seconds: f32) -> Voice {
    old.fade_out(seconds);
    mixer.play_stream(
        next,
        PlayOptions {
            bus: Some(mixer.music()),
            fade_in: seconds,
            ..Default::default()
        },
    )
}
```

## Custom streams

To play audio the game produces while it plays, such as procedural music or
a format Yowie does not decode, implement `audio::Stream` and pass it to
`Mixer::play_stream`. The stream moves to the audio thread.

| `Stream` method | Contract |
|---|---|
| `read(&mut self, out: &mut [f32]) -> usize` | fills `out` with interleaved stereo frames at the mixer's rate and returns how many frames it wrote. Returning fewer than `out.len() / 2` ends the stream. A stream that falls behind writes silence and reports those frames as written |
| `seek(&mut self, seconds: f64) -> bool` | optional; moves to `seconds` and returns true, or returns false when the stream cannot seek. The default returns false |

`read` runs on the audio thread. It must not block, must not allocate once
it is running, and must not take a lock that another thread can hold for
long. The voice's pitch applies to a stream. The mixer resamples what `read`
returns, and reads ahead of playback in blocks of 1024 frames.

This stream plays a rising and falling siren for half a second, rendered
offline:

```rust
use yowie::audio::{Mixer, PlayOptions, Stream};

struct Siren {
    rate: f32,
    phase: f32,
    t: f32,
    frames_left: usize,
}

impl Stream for Siren {
    fn read(&mut self, out: &mut [f32]) -> usize {
        let frames = (out.len() / 2).min(self.frames_left);
        for frame in out[..frames * 2].chunks_exact_mut(2) {
            let hz = 600.0 + 300.0 * (self.t * 2.0).sin();
            self.phase = (self.phase + hz / self.rate).fract();
            let s = (self.phase * std::f32::consts::TAU).sin() * 0.3;
            frame[0] = s;
            frame[1] = s;
            self.t += 1.0 / self.rate;
        }
        self.frames_left -= frames;
        frames
    }
}

fn main() -> yowie::audio::Result<()> {
    let (mixer, mut renderer) = Mixer::offline(48_000)?;
    let siren = Siren {
        rate: 48_000.0,
        phase: 0.0,
        t: 0.0,
        frames_left: 24_000,
    };
    let voice = mixer.play_stream(siren, PlayOptions::default());
    let out = renderer.render_frames(48_000);
    assert!(voice.is_finished());
    assert!(out.iter().any(|s| s.abs() > 0.1));
    Ok(())
}
```

`Voice::position` on a stream counts source time since the voice started or
last sought. It includes pitch and silence written while the stream was
behind, and does not wrap at loops.

## Filters and reverb

The mixer has these effects:

| Effect | Where |
|---|---|
| low-pass filter | per voice (`PlayOptions::low_pass`, `Voice::set_low_pass`) and per bus (`Bus::set_low_pass`) |
| reverb | one shared reverb, an optional reverb per bus, and reverb zones in the world |
| occlusion | per voice; see [Occlusion](#occlusion) |
| Doppler shift | per positional voice; see [Doppler](#doppler) |

### Low-pass filter

To muffle a voice, give it a low-pass cutoff in Hz. To muffle a whole group,
set the cutoff on its bus. The bus setter takes a plain number, and zero
removes the filter:

```rust
use yowie::audio::Mixer;

fn open_pause_menu(mixer: &Mixer) {
    mixer.effects().set_paused(true);
    mixer.music().set_low_pass(800.0);
}

fn close_pause_menu(mixer: &Mixer) {
    mixer.effects().set_paused(false);
    mixer.music().set_low_pass(0.0);
}
```

### Reverb

To add a room's echo, configure the shared reverb with `Mixer::set_reverb`
and give voices a send level. A voice's `reverb` send adds a copy of the
voice, scaled by the send, into the reverb; the voice's direct sound is
unchanged. The reverb is a stereo Freeverb-style design (eight comb filters
and four allpass filters per channel).

| `ReverbSettings` field | Zero means | Meaning |
|---|---|---|
| `room_size` | 0.5 | how long the tail lasts, 0 to 1 |
| `damping` | 0.5 | how quickly high frequencies die away, 0 to 1 |
| `width` | 1.0 | stereo spread, 0 to 1 |
| `wet` | reverb off | level of the reverb in the mix, 0 to 1 |

Values outside 0 to 1 are clamped. `ReverbSettings::default()` has a zero
`wet`, so it turns the reverb off. The mixer builds the reverb only while
something needs it. `ReverbSettings::with_defaults()` fills in the zero
fields with the values above.

```rust
use yowie::audio::{Mixer, PlayOptions, ReverbSettings, Sound};

fn enter_cathedral(mixer: &Mixer) {
    mixer.set_reverb(ReverbSettings {
        room_size: 0.9,
        damping: 0.3,
        wet: 0.5,
        ..Default::default()
    });
}

fn play_bell(mixer: &Mixer, bell: &Sound) {
    mixer.play(
        bell,
        PlayOptions {
            reverb: 0.8,
            bus: Some(mixer.effects()),
            ..Default::default()
        },
    );
}
```

### Bus reverb

To give one group of voices a room of its own, call `Bus::set_reverb`. The
voices on that bus send into the bus's reverb instead of the shared one, and
the reverb's output joins the master bus. Settings with a zero `wet` remove
the bus reverb. A radio or intercom bus is a typical use:

```rust
use yowie::audio::{Bus, Mixer, ReverbSettings};

fn intercom_bus(mixer: &Mixer) -> yowie::audio::Result<Bus> {
    let bus = mixer.new_bus("intercom")?;
    bus.set_low_pass(3000.0);
    bus.set_reverb(ReverbSettings {
        room_size: 0.2,
        damping: 0.7,
        wet: 0.3,
        ..Default::default()
    });
    Ok(bus)
}
```

### Reverb zones

To change the shared reverb as the listener moves between spaces, call
`Mixer::set_reverb_zones` with a list of `ReverbZone` spheres. The call
replaces the whole list; pass an empty slice to clear it.

| `ReverbZone` field | Meaning |
|---|---|
| `center` | sphere centre in world units |
| `radius` | sphere radius in world units; a zone with a radius of zero or less is ignored |
| `fade` | distance in from the edge over which the zone blends in; zero or less means the whole radius |
| `settings` | the reverb at full zone strength |

While the [listener](#the-listener) is inside a zone, the zone's settings
replace the shared reverb's, blended in over `fade`. Where zones overlap,
the one the listener is furthest inside wins. When the shared reverb is off,
the zone keeps its own settings and only its `wet` level fades in. Zones
change the shared reverb only; bus reverbs ignore them.

```rust
use yowie::Vec3;
use yowie::audio::{Mixer, ReverbSettings, ReverbZone};

fn set_cave_zones(mixer: &Mixer) {
    mixer.set_reverb_zones(&[ReverbZone {
        center: Vec3::new(40.0, 0.0, -12.0),
        radius: 15.0,
        fade: 4.0,
        settings: ReverbSettings {
            room_size: 0.95,
            damping: 0.2,
            wet: 0.8,
            ..Default::default()
        },
    }]);
}
```

`cargo run --example audio -- --zone` puts a zone over the left half of the
window and moves the listener with the mouse.

## Spatial audio

A positional voice is heard from a place in the world. The mixer pans it by
its direction from the listener and lowers its volume with distance. With
Doppler on, motion shifts its pitch, and an occlusion amount that the game
sets muffles it.

Use one unit for the world, such as metres in a 3D game or pixels in a 2D
game. Positions, distances, velocities and the speed of sound all use it.

### The listener

To set where sound is heard from, call `Mixer::set_listener` with a
`Listener`. Call it every frame when the camera moves.

| `Listener` field | Default | Meaning |
|---|---|---|
| `position` | origin | the listener's position |
| `forward` | -Z | the facing direction; need not be normalised, and zero becomes -Z |
| `up` | +Y | the head-up direction; need not be normalised, and zero becomes +Y |
| `velocity` | zero | world units per second, for Doppler |

`Listener::from_transform(&transform)` takes the position and facing from a
`Transform`. `Listener::new_2d(x, y)` places a listener for a 2D game on the
XY plane, with y down and facing into the screen.

For a 3D camera:

```rust
use yowie::Vec3;
use yowie::audio::{Listener, Mixer};
use yowie::gfx::scene::Camera3D;

fn hear_from_camera(mixer: &Mixer, camera: &Camera3D, velocity: Vec3) {
    mixer.set_listener(Listener {
        position: camera.position,
        forward: camera.forward(),
        up: camera.up,
        velocity,
    });
}
```

### Positional voices

To place a voice in the world, set `PlayOptions::positional` to a
`Positional`. `Positional::at(position)` gives one with every other field at
its default.

| `Positional` field | Default | Meaning |
|---|---|---|
| `position` | origin | source position |
| `velocity` | zero | world units per second, for Doppler |
| `min_distance` | 1.0 | radius of full volume |
| `max_distance` | 100.0 | distance of silence |
| `relative` | false | read `position`, `velocity` and `direction` in the listener's frame (x right, y up, -z forward) |
| `direction` | -Z | the direction the source faces, for its cone |
| `cone` | `Cone::OMNI` | directional falloff; see [Cones](#cones) |
| `attenuation` | `Attenuation::default()` | distance falloff; see [Distance attenuation](#distance-attenuation) |

A `min_distance` that is not positive becomes 1, and a `max_distance` that is
not above it becomes the larger of twice `min_distance` and 100.

A positional voice ignores `pan`. It is panned by its direction from the
listener; a source to one side keeps some level in the far ear, and inside
`min_distance` the pan narrows toward the centre.

This is how the audio example places a source in a 2D game measured in
pixels:

```rust
use yowie::Vec3;
use yowie::audio::{Listener, Mixer, PlayOptions, Positional, Sound, Voice};

fn play_at(mixer: &Mixer, sound: &Sound, x: f32, y: f32) -> Voice {
    mixer.play(
        sound,
        PlayOptions {
            positional: Some(Positional {
                min_distance: 60.0,
                max_distance: 500.0,
                ..Positional::at(Vec3::new(x, y, 0.0))
            }),
            ..Default::default()
        },
    )
}

fn centre_listener(mixer: &Mixer, x: f32, y: f32) {
    mixer.set_listener(Listener::new_2d(x, y));
}
```

To move a source, call `Voice::set_position` each frame. These setters change
a voice's place in the world, and each one makes a voice positional if it
was not:

| Method | Effect |
|---|---|
| `set_position(p)` | moves the source |
| `set_velocity(v)` | sets the velocity for Doppler; it changes only the pitch, so move the voice with `set_position` |
| `set_distance_range(min, max)` | sets the full-volume radius and the distance of silence; ignored unless `0 < min < max` |
| `set_direction(d)` | sets the facing direction; ignored when zero or not finite |
| `set_cone(cone)` | sets the source cone |
| `set_attenuation(a)` | sets the distance model |
| `set_relative_to_listener(bool)` | reads position, velocity and direction in the listener's frame |

A voice relative to the listener moves with the listener, which suits sounds
attached to the player, such as a cockpit hum or footsteps.

### Distance attenuation

To choose how volume falls with distance, set `Positional::attenuation` or
call `Voice::set_attenuation`. `Attenuation::new(model, rolloff)` fails when
`rolloff` is negative or not finite, and a `rolloff` of zero becomes 1.

Every model plays at full volume within `min_distance` and is silent at
`max_distance` and beyond, except `None`. Between them, with `d` the
distance, `x = (d - min) / (max - min)` and `r` the rolloff:

| `AttenuationModel` | Gain between `min_distance` and `max_distance` |
|---|---|
| `Standard` (default) | `min / (min + r * (d - min)) * (1 - x)` (inverse distance with a linear cutoff) |
| `Inverse` | `min / (min + r * (d - min))` |
| `Linear` | `1 - r * x`, not below 0 |
| `Exponential` | `(d / min)` to the power `-r` |
| `None` | 1 at every distance |

```rust
use yowie::Vec3;
use yowie::audio::{Attenuation, AttenuationModel, Positional};

fn waterfall(at: Vec3) -> yowie::audio::Result<Positional> {
    Ok(Positional {
        min_distance: 5.0,
        max_distance: 80.0,
        attenuation: Attenuation::new(AttenuationModel::Linear, 1.0)?,
        ..Positional::at(at)
    })
}
```

### Cones

To make a source louder in front than behind, give it a cone.
`Cone::new(inner, outer, outer_gain)` takes full cone angles in radians, with
`0 <= inner <= outer <= 2 * PI`, and a gain from 0 to 1. A listener inside the
inner angle hears full volume, one outside the outer angle hears
`outer_gain`, and the gain changes linearly between them. `Cone::new` fails
when the values are out of range. `Cone::OMNI` is the default.

```rust
use std::f32::consts::PI;
use yowie::Vec3;
use yowie::audio::{Cone, Positional};

fn loudspeaker(at: Vec3, facing: Vec3) -> yowie::audio::Result<Positional> {
    Ok(Positional {
        direction: facing,
        cone: Cone::new(PI / 2.0, PI * 1.5, 0.2)?,
        ..Positional::at(at)
    })
}
```

### Doppler

Doppler shift is off by default. To turn it on, call
`Mixer::set_doppler(factor)`. A factor of 0 turns it off and 1 is physical.
Set the speed of sound in world units per second with
`Mixer::set_speed_of_sound`; the default is 343, which suits a game measured
in metres. The shift comes
from the source's velocity (`Positional::velocity` or `Voice::set_velocity`)
and the listener's, along the line between them.

In a 2D game measured in pixels, raise the speed of sound. The audio example
uses 3000, which keeps the shift near a semitone:

```rust
use yowie::audio::Mixer;

fn pixel_doppler(mixer: &Mixer) {
    mixer.set_doppler(1.0);
    mixer.set_speed_of_sound(3000.0);
}
```

### Occlusion

To muffle a source behind a wall, set its occlusion from 0 (clear) to 1
(fully blocked) with `PlayOptions::occlusion` or `Voice::set_occlusion`.
At 1 the voice is 20 dB down and low-passed at 400 Hz. Occlusion works on any
voice, positional or not.

Yowie does not work out occlusion. Measure it in the game, for example with
a ray cast from the listener to the source through the
[physics world](physics.md), and set the result each frame:

```rust
use yowie::audio::Voice;

/// `walls` is how many walls the ray from the listener to the source crossed.
fn occlude(voice: &Voice, walls: usize) {
    voice.set_occlusion((walls as f32 * 0.5).min(1.0));
}
```

### Binaural rendering

To render positional voices for each ear through a head model, call
`Mixer::set_spatial` with `binaural` set:

```rust
use yowie::audio::{Mixer, SpatialSettings};

fn use_binaural(mixer: &Mixer, on: bool) {
    mixer.set_spatial(SpatialSettings {
        binaural: on,
        head_radius: 0.0,
    });
}
```

The head model delays the far ear, lowers its level and dulls its high
frequencies by the source's angle, and lifts or lowers the high band as the
source rises or falls. It computes these cues from the source's direction
and the head radius. A stereo source is mixed to mono before the model.
`head_radius` is in metres whatever the world's units; zero or less means
0.0875, and the most is 0.3. Voices that are not positional play as before.

## Microphone capture

To record from a microphone, open a `Capture`:

| Call | Effect |
|---|---|
| `list_inputs()` | lists input devices as `DeviceInfo` (`id`, `name`, `is_default`) |
| `Capture::open_default()` | opens the default input device |
| `Capture::open(id_or_name)` | opens a device from `list_inputs` by id or by name |
| `rate()`, `channels()` | the device's sample rate and channel count |
| `read(&mut buf)` | copies waiting interleaved samples into `buf` and returns how many; zero when none are waiting |
| `level()` | the peak and RMS level, 0 to 1, of the most recent block |
| `dropped_samples()` | samples lost because the game did not read fast enough |

Samples arrive at the device's rate and channel count. The device callback
copies them into a fixed set of blocks, about 1.5 s of mono at 44.1 kHz, and
drops audio when no free block is left. The level meter updates only with
blocks that were kept, so read every frame even when you only want the
level, as the audio example does.

Open the microphone only when the player asks for it. The audio example
opens it only with `--mic`, and never in a headless run (`ctx.headless()`).

To record a clip, wrap the capture in a `Recorder`. It records into memory or
into a 16-bit WAV file, up to a length you give, and stops by itself at that
limit.

| `Recorder` method | Effect |
|---|---|
| `Recorder::to_memory(capture, max_seconds)` | records into memory |
| `Recorder::to_wav_file(capture, path, max_seconds)` | records into a WAV file |
| `update()` | moves waiting audio into the recording; call it every frame |
| `is_full()` | true once the length limit is reached |
| `seconds()` | seconds recorded so far |
| `level()` | the input level, for a meter |
| `finish()` | stops; returns `Some(pcm)` for a memory recording, or finishes the file and returns `None` |

```rust,no_run
use yowie::audio::{Capture, Mixer, PlayOptions, Recorder, Voice};

/// Records up to ten seconds from the default microphone, then plays it back.
struct VoiceMemo {
    recorder: Option<Recorder>,
}

impl VoiceMemo {
    fn start(&mut self) -> yowie::audio::Result<()> {
        let capture = Capture::open_default()?;
        self.recorder = Some(Recorder::to_memory(capture, 10.0)?);
        Ok(())
    }

    /// Call once per frame while recording.
    fn update(&mut self) -> yowie::audio::Result<()> {
        if let Some(recorder) = &mut self.recorder {
            recorder.update()?;
        }
        Ok(())
    }

    fn stop_and_play(&mut self, mixer: &Mixer) -> yowie::audio::Result<Option<Voice>> {
        let Some(recorder) = self.recorder.take() else {
            return Ok(None);
        };
        let Some(pcm) = recorder.finish()? else {
            return Ok(None);
        };
        let sound = mixer.sound(&pcm)?;
        Ok(Some(mixer.play(&sound, PlayOptions::default())))
    }
}
```

`Mixer::sound` accepts one or two channels, so a recording from a device
with more channels needs mixing down before it can play.

## Output devices

The engine's mixer always plays on the system's default output device. To
play on another device, open an `Output` for it and play through its mixer:

| Call | Effect |
|---|---|
| `list_outputs()` | lists output devices as `DeviceInfo` (`id`, `name`, `is_default`) |
| `Output::open_default()` | opens the default output device |
| `Output::open(id_or_name)` | opens a device from `list_outputs` by id or by name |
| `mixer()` | the mixer this output plays |
| `device_name()`, `channels()` | the device in use |
| `pause()`, `resume()` | pauses and resumes the device; playback positions stop while paused |

The device plays only while the `Output` is alive; dropping it stops the
device. The mixer runs at the device's own sample rate, so make sounds for
it with its own `Mixer::sound`. A mono device gets the stereo mix averaged,
and a device with more than two channels gets the mix on the first two.

The engine does not update a mixer you open yourself. Call its
`Mixer::update` once per frame so its callbacks run and finished voices are
freed.

```rust,no_run
use yowie::audio::{Output, list_outputs};

fn open_output(name: &str) -> yowie::audio::Result<Output> {
    for device in list_outputs()? {
        let mark = if device.is_default { " (default)" } else { "" };
        println!("{}: {}{}", device.id, device.name, mark);
    }
    Output::open(name)
}
```

## Offline mixing and headless runs

To mix without a device, make the mixer with `Mixer::offline(rate)`. It
returns the mixer and a `Renderer`. Playback advances only when you render:

| `Renderer` method | Effect |
|---|---|
| `render(&mut out)` | mixes interleaved stereo frames into `out`; an odd trailing sample is left untouched |
| `render_frames(frames)` | mixes `frames` stereo frames into a new `Vec<f32>` |

The mixer's output is clamped to -1 to 1, and a sample that is not finite
becomes silence. Keep the sum of loud voices below full scale to avoid
clipping.

The engine uses an offline mixer at 48 kHz for headless runs, for
`Config::with_audio(false)` and when no device opens. Each frame it renders
the game time that frame advanced, at most a quarter of a second, and
discards the result. Under `YOWIE_FIXED_CLOCK=1` that is one fixed step per
frame, so a headless run starts and finishes voices at the same frames every
time.

An offline mixer suits unit tests of game audio logic. Call `Mixer::update`
yourself to run callbacks:

```rust
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use yowie::audio::{Mixer, Pcm, PlayOptions};

fn main() -> yowie::audio::Result<()> {
    let (mixer, mut renderer) = Mixer::offline(48_000)?;
    let hum = mixer.sound(&Pcm::sine(110.0, 1.0, 48_000))?;
    let voice = mixer.play(
        &hum,
        PlayOptions {
            looping: true,
            ..Default::default()
        },
    );
    let done = Arc::new(AtomicBool::new(false));
    let flag = done.clone();
    voice.on_done(move || flag.store(true, Ordering::SeqCst));

    voice.fade_out(0.1);
    renderer.render_frames(9_600);
    assert!(voice.is_finished());

    // The callback runs on the thread that calls update.
    mixer.update();
    assert!(done.load(Ordering::SeqCst));
    Ok(())
}
```

To write audio to a file, render it and save the `Pcm` with
`Pcm::save_wav(path)`, or `Pcm::write_wav(&mut writer)` for any writer. Both
write 16-bit PCM WAV and clip samples outside -1 to 1.

```rust,no_run
use yowie::audio::{Mixer, Pcm, PlayOptions, ReverbSettings};

/// Renders `dry` through a large reverb, with one second of tail, into a WAV file.
fn bake_reverb(dry: &Pcm, path: &str) -> yowie::audio::Result<()> {
    let rate = 44_100;
    let (mixer, mut renderer) = Mixer::offline(rate)?;
    mixer.set_reverb(ReverbSettings {
        room_size: 0.9,
        wet: 0.6,
        ..Default::default()
    });
    let sound = mixer.sound(dry)?;
    mixer.play(
        &sound,
        PlayOptions {
            reverb: 1.0,
            ..Default::default()
        },
    );
    let samples = renderer.render_frames(sound.frames() + rate as usize);
    Pcm {
        samples,
        channels: 2,
        rate,
    }
    .save_wav(path)
}
```

The `play` example in the audio crate plays any supported file, or mixes it
offline into a WAV file with `--dump`:

```bash
cargo run -p yowie-audio --example play -- song.ogg
cargo run -p yowie-audio --example play -- song.xm --dump song.wav --seconds 30
```

## Tracker music

The `audio::tracker` module plays tracker modules:

| Format | Loader | Notes |
|---|---|---|
| ProTracker MOD | `load_mod` | 4 to 32 channels |
| ScreamTracker 3 S3M | `load_s3m` | |
| FastTracker 2 XM | `load_xm` | |
| Impulse Tracker IT | `load_it` | compressed samples included |

To load a module, call `Module::load(&bytes)`. It detects the format and
calls the matching loader; a file it cannot parse fails with
`Error::Module`.

To play it, make a `Player` with the mixer's rate and pass it to
`Mixer::play_stream`. A `Player` is a `Stream`, so the module plays as a
voice: on a bus, with volume, fades and pause. Playback is deterministic, so
a module renders the same samples on every platform.

| `PlayerOptions` field | Default | Meaning |
|---|---|---|
| `looping` | false | restart at the module's restart position when the order list ends |
| `amiga_filter` | false | enable the ProTracker LED low-pass filter (effects E00 and E01) |
| `cubic` | false | interpolate samples with a four-point Hermite curve instead of linearly, which softens aliasing |

```rust,no_run
use std::sync::Arc;
use yowie::audio::PlayOptions;
use yowie::audio::tracker::{Module, Player, PlayerControl, PlayerOptions};
use yowie::{Context, GameResult};

fn start_module(ctx: &Context, path: &str) -> GameResult<PlayerControl> {
    let bytes = ctx.assets.vfs().read(path)?;
    let module = Arc::new(Module::load(&bytes)?);
    let options = PlayerOptions {
        looping: true,
        ..Default::default()
    };
    let player = Player::with_options(module, ctx.audio.rate(), options);
    let control = player.control();
    ctx.audio.play_stream(
        player,
        PlayOptions {
            bus: Some(ctx.audio.music()),
            ..Default::default()
        },
    );
    Ok(control)
}
```

Keep the `PlayerControl` from `Player::control` to control the song after it
has moved to the mixer:

| `PlayerControl` method | Effect |
|---|---|
| `position()` | the current song position (an index into the order list) and row |
| `length()` | the number of song positions |
| `rows(order)` | the number of rows at a song position, or 0 outside the song |
| `channels()` | the number of pattern channels |
| `seek(order, row)` | jumps there and plays on, cutting what was sounding; speed, tempo and global volume stay as they are, and seeking a finished song starts it again |
| `mute(channel, bool)` / `muted(channel)` | silences a channel while the song plays on; notes it starts keep running unheard, so unmuting brings them back mid-note |
| `solo(channel, bool)` / `soloed(channel)` | while any channel is soloed, only soloed channels are heard |
| `finished()` | true once a song that does not loop has ended |

Mute and solo cover channels 0 to 63. `Voice::seek` does nothing on a
tracker voice; use `PlayerControl::seek`. Muting channels lets the music
react to the game, for example by bringing in a drum channel during a fight.

A `Module` carries the song's data: `title`, `format` (`Format::name()` gives
"MOD", "S3M", "XM" or "IT"), `channels`, `orders`, `patterns`, `samples`,
`instruments`, and the initial `speed` and `tempo`. Each pattern holds
`rows[row][channel]` cells with a `note`, `instrument`, volume column and
effect. To animate visuals in time with the music, read the row the player
is on:

```rust
use yowie::audio::tracker::{Module, NOTE_NONE, PlayerControl};

/// The channels that start a note on the current row.
fn channels_hit(control: &PlayerControl, module: &Module) -> Vec<usize> {
    let (order, row) = control.position();
    let Some(cells) = module
        .orders
        .get(order)
        .and_then(|&pattern| module.patterns.get(pattern))
        .and_then(|pattern| pattern.rows.get(row))
    else {
        return Vec::new();
    };
    cells
        .iter()
        .enumerate()
        .filter(|(_, cell)| cell.note != NOTE_NONE)
        .map(|(channel, _)| channel)
        .collect()
}
```

`position()` reports where the player has rendered to. That is ahead of what
is heard by the mixer's 1024-frame stream buffer and the device's buffer.

To listen to a module from the command line, run
`cargo run -p yowie-audio --example play -- song.mod`.

## Threading and latency

- Calls on `Mixer` and `Voice` queue messages for the audio thread and
  return at once. Most `Bus` settings are shared values that the audio
  thread reads. No call waits for the audio thread. The audio thread applies
  waiting messages at the start of each 256-frame chunk.
- The message queue holds 4096 messages. When it is full, a change is
  dropped with a logged warning, and a `play` returns a voice that is
  already finished. Call setters when a value changes, not repeatedly for the
  same value.
- The audio thread does not wait for the game and does not allocate once
  running. New voices and buses arrive already built, and finished voices
  are sent back and dropped on the thread that calls `Mixer::update`.
- `Voice::is_playing` turns false at once when you stop a voice.
  `is_finished` and `position` follow the audio thread, as of the last chunk
  it mixed.
- An `Output` opens its device with the device's default sample rate,
  channel count and buffer size. `Mixer::rate` reports the rate. A change
  is heard after the next device callback mixes it and the device plays
  that buffer, so the system's buffer size sets the latency.
- `Music` decodes on its own thread and keeps about 0.7 s queued. A tracker
  `Player` and any custom `Stream` run on the audio thread. A `Capture`
  receives samples on the device's thread and hands them to the game in
  blocks.
- `Voice::on_done` callbacks run on the thread that calls `Mixer::update`,
  never on the audio thread.

## See also

- `cargo run --example audio`: the tour, with the flags listed at the top.
- `cargo run -p yowie-audio --example play -- <file>`: plays or renders any
  supported file, tracker modules included.
- [Building Tetris](tetris.md) plays synthesised sounds from an ECS system
  that reads game events.
- [Physics](physics.md) for ray casts that measure occlusion.
- [The debug console](console.md) for the Audio panel.
- `cargo doc --open` for the full `yowie::audio` reference.
