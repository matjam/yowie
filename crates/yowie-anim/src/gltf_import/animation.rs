//! Converts glTF animations to clips.

use gltf::animation::util::ReadOutputs;
use gltf::animation::{Interpolation, Property};
use yowie_core::{Quat, Vec3};

use crate::clip::Clip;
use crate::curve::{Curve, Interp, Key};
use crate::error::{AnimError, Result};
use crate::morph::weights_channel;
use crate::value::Channel;

fn bad(msg: &str) -> AnimError {
    AnimError::Unsupported(msg.into())
}

/// Builds keys from times and outputs. Cubic spline outputs hold an in
/// tangent, a value and an out tangent per key.
fn keys<T: Clone>(times: &[f32], values: &[T], interp: Interpolation) -> Result<Vec<Key<T>>> {
    match interp {
        Interpolation::CubicSpline => {
            if values.len() != times.len() * 3 {
                return Err(bad("cubic spline output count is not three per key"));
            }
            Ok(times
                .iter()
                .zip(values.as_chunks::<3>().0)
                .map(|(t, v)| Key::cubic(*t, v[0].clone(), v[1].clone(), v[2].clone()))
                .collect())
        }
        other => {
            if values.len() != times.len() {
                return Err(bad("output count differs from input count"));
            }
            let step = other == Interpolation::Step;
            Ok(times
                .iter()
                .zip(values)
                .map(|(t, v)| {
                    if step {
                        Key::step(*t, v.clone())
                    } else {
                        Key::new(*t, v.clone())
                    }
                })
                .collect())
        }
    }
}

/// Builds one clip from a glTF animation.
pub(super) fn build_clip(
    animation: &gltf::Animation,
    names: &[String],
    buffers: &[gltf::buffer::Data],
) -> Result<Clip> {
    let name = animation
        .name()
        .map_or_else(|| format!("animation{}", animation.index()), str::to_string);
    let mut clip = Clip::new(name);
    for channel in animation.channels() {
        let reader = channel.reader(|b| buffers.get(b.index()).map(|d| &d.0[..]));
        let times: Vec<f32> = reader
            .read_inputs()
            .ok_or_else(|| bad("channel has no input"))?
            .collect();
        let interp = channel.sampler().interpolation();
        let target = names[channel.target().node().index()].clone();
        let outputs = reader
            .read_outputs()
            .ok_or_else(|| bad("channel has no output"))?;
        let ch = match (channel.target().property(), outputs) {
            (Property::Translation, ReadOutputs::Translations(it)) => {
                let v: Vec<Vec3> = it.map(Vec3::from_array).collect();
                Channel::Translation(Curve::new(keys(&times, &v, interp)?))
            }
            (Property::Scale, ReadOutputs::Scales(it)) => {
                let v: Vec<Vec3> = it.map(Vec3::from_array).collect();
                Channel::Scale(Curve::new(keys(&times, &v, interp)?))
            }
            (Property::Rotation, ReadOutputs::Rotations(it)) => {
                let v: Vec<Quat> = it.into_f32().map(Quat::from_array).collect();
                Channel::Rotation(Curve::new(keys(&times, &v, interp)?))
            }
            (Property::MorphTargetWeights, ReadOutputs::MorphTargetWeights(it)) => {
                morph_channel(&times, it.into_f32().collect(), interp)?
            }
            _ => return Err(bad("channel property and output type disagree")),
        };
        clip = clip.with_track(target, ch);
    }
    Ok(clip)
}

/// Splits flat morph weights (targets per key, three groups per key for
/// cubic splines) into one curve per target.
fn morph_channel(times: &[f32], flat: Vec<f32>, interp: Interpolation) -> Result<Channel> {
    let per_key = if interp == Interpolation::CubicSpline {
        3
    } else {
        1
    };
    if times.is_empty() || !flat.len().is_multiple_of(times.len() * per_key) {
        return Err(bad("morph weight count does not divide into keys"));
    }
    let targets = flat.len() / (times.len() * per_key);
    if interp != Interpolation::CubicSpline {
        let rows: Vec<Vec<f32>> = flat.chunks(targets.max(1)).map(<[f32]>::to_vec).collect();
        let kind = if interp == Interpolation::Step {
            Interp::Step
        } else {
            Interp::Linear
        };
        return Ok(weights_channel(times, &rows, kind));
    }
    let curves = (0..targets)
        .map(|k| {
            let at = |key: usize, part: usize| flat[(key * 3 + part) * targets + k];
            Curve::new(
                times
                    .iter()
                    .enumerate()
                    .map(|(i, t)| Key::cubic(*t, at(i, 0), at(i, 1), at(i, 2)))
                    .collect(),
            )
        })
        .collect();
    Ok(Channel::Weights(curves))
}
