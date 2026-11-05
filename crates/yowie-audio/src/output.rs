//! Playback through a `cpal` output device.

use crate::core::Core;
use crate::error::{Error, Result};
use crate::mixer::Mixer;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Device, FromSample, SampleFormat, SizedSample, Stream, StreamConfig};

/// A device that can play audio.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeviceInfo {
    /// Stable identifier; pass to [`Output::open`].
    pub id: String,
    /// Human-readable name.
    pub name: String,
    /// Whether the host reports this as its default device.
    pub is_default: bool,
}

pub(crate) fn describe(d: &Device, default_id: Option<&str>) -> DeviceInfo {
    let id = d.id().map(|i| i.to_string()).unwrap_or_default();
    let name = d.description().map(|x| x.name().to_string()).unwrap_or_else(|_| id.clone());
    let is_default = default_id.is_some_and(|x| x == id);
    DeviceInfo { id, name, is_default }
}

/// Lists the output devices of the default host.
pub fn list_outputs() -> Result<Vec<DeviceInfo>> {
    let host = cpal::default_host();
    let default_id = host.default_output_device().and_then(|d| d.id().ok()).map(|i| i.to_string());
    Ok(host.output_devices()?.map(|d| describe(&d, default_id.as_deref())).collect())
}

/// Finds an output device by id or by name.
pub(crate) fn find_device(inputs: bool, which: &str) -> Result<Device> {
    let host = cpal::default_host();
    let devices: Vec<Device> =
        if inputs { host.input_devices()?.collect() } else { host.output_devices()?.collect() };
    devices
        .into_iter()
        .find(|d| {
            let info = describe(d, None);
            info.id == which || info.name == which
        })
        .ok_or(Error::NoDevice)
}

/// A running output stream that drives a [`Mixer`].
///
/// Keep the `Output` alive for as long as sound should play; dropping it stops the
/// device. The mixer is converted to the device's sample rate and layout: the mixer runs
/// at the device rate, mono devices get the stereo mix averaged, and devices with more
/// than two channels get the mix on the first two.
pub struct Output {
    mixer: Mixer,
    stream: Stream,
    device_name: String,
    channels: u16,
}

impl Output {
    /// Opens the system's default output device.
    pub fn open_default() -> Result<Self> {
        let device = cpal::default_host().default_output_device().ok_or(Error::NoDevice)?;
        Self::build(&device)
    }

    /// Opens a device chosen from [`list_outputs`] by id or name.
    pub fn open(device: &str) -> Result<Self> {
        Self::build(&find_device(false, device)?)
    }

    fn build(device: &Device) -> Result<Self> {
        let supported = device.default_output_config()?;
        let config = supported.config();
        let (mixer, core) = Mixer::with_core(config.sample_rate)?;
        let stream = match supported.sample_format() {
            SampleFormat::F32 => run::<f32>(device, config.clone(), core)?,
            SampleFormat::F64 => run::<f64>(device, config.clone(), core)?,
            SampleFormat::I16 => run::<i16>(device, config.clone(), core)?,
            SampleFormat::U16 => run::<u16>(device, config.clone(), core)?,
            SampleFormat::I32 => run::<i32>(device, config.clone(), core)?,
            SampleFormat::U32 => run::<u32>(device, config.clone(), core)?,
            SampleFormat::I8 => run::<i8>(device, config.clone(), core)?,
            SampleFormat::U8 => run::<u8>(device, config.clone(), core)?,
            other => return Err(Error::Unsupported(format!("sample format {other}"))),
        };
        stream.play()?;
        let name = describe(device, None).name;
        log::info!("audio output {name}: {} Hz, {} channels", config.sample_rate, config.channels);
        Ok(Self { mixer, stream, device_name: name, channels: config.channels })
    }

    /// The mixer this output plays. Clone it freely.
    pub fn mixer(&self) -> Mixer {
        self.mixer.clone()
    }

    /// The name of the device in use.
    pub fn device_name(&self) -> &str {
        &self.device_name
    }

    /// The device's channel count.
    pub fn channels(&self) -> u16 {
        self.channels
    }

    /// Pauses the device. Playback positions stop advancing.
    pub fn pause(&self) -> Result<()> {
        self.stream.pause()?;
        Ok(())
    }

    /// Resumes a paused device.
    pub fn resume(&self) -> Result<()> {
        self.stream.play()?;
        Ok(())
    }
}

/// Frames mixed per pass of the device callback; larger callbacks are mixed in pieces.
const PASS_FRAMES: usize = 2048;

fn run<T>(device: &Device, config: StreamConfig, mut core: Core) -> Result<Stream>
where
    T: SizedSample + FromSample<f32>,
{
    let channels = usize::from(config.channels).max(1);
    let mut stereo = vec![0.0f32; PASS_FRAMES * 2];
    let stream = device.build_output_stream(
        config,
        move |data: &mut [T], _| {
            for piece in data.chunks_mut(PASS_FRAMES * channels) {
                let frames = piece.len() / channels;
                let buf = &mut stereo[..frames * 2];
                core.render(buf);
                write_frames(piece, buf, channels);
            }
        },
        |e| log::warn!("audio stream error: {e}"),
        None,
    )?;
    Ok(stream)
}

/// Converts mixed stereo to a device layout of `channels` interleaved channels.
fn write_frames<T: SizedSample + FromSample<f32>>(out: &mut [T], stereo: &[f32], channels: usize) {
    for (frame, s) in out.chunks_exact_mut(channels).zip(stereo.chunks_exact(2)) {
        match channels {
            1 => frame[0] = T::from_sample((s[0] + s[1]) * 0.5),
            _ => {
                frame[0] = T::from_sample(s[0]);
                frame[1] = T::from_sample(s[1]);
                for extra in &mut frame[2..] {
                    *extra = T::from_sample(0.0);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_conversion() {
        let stereo = [0.5, -0.5, 1.0, 0.0];
        let mut mono = [0.0f32; 2];
        write_frames(&mut mono, &stereo, 1);
        assert_eq!(mono, [0.0, 0.5]);
        let mut six = [9.0f32; 12];
        write_frames(&mut six, &stereo, 6);
        assert_eq!(&six[..6], &[0.5, -0.5, 0.0, 0.0, 0.0, 0.0]);
        assert_eq!(&six[6..8], &[1.0, 0.0]);
        let mut short = [0i16; 4];
        write_frames(&mut short, &stereo, 2);
        assert_eq!(short[2], i16::MAX);
    }
}
