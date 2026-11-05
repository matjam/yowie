// Auto exposure. `cs_histogram` counts the picture's pixels into 64 bins
// of log2 luminance (bin 0 holds black); `cs_adapt` averages the log
// luminance of the pixels between the 40th and 95th percentile, moves the
// adapted luminance towards it by this frame's share, and stores the
// exposure that maps it to mid-grey (0.18) plus the compensation, then
// empties the bins for the next frame. The composite and bloom multiply
// by the stored exposure.

struct ExposureState {
    exposure: f32,
    luminance: f32,
    valid: f32,
    pad: f32,
    bins: array<atomic<u32>, 64>,
}

@group(1) @binding(0) var picture: texture_2d<f32>;
@group(1) @binding(1) var<storage, read_write> state: ExposureState;

const MIN_LOG: f32 = -10.0;
const MAX_LOG: f32 = 6.0;

var<workgroup> local_bins: array<atomic<u32>, 64>;

@compute @workgroup_size(16, 16)
fn cs_histogram(@builtin(global_invocation_id) id: vec3<u32>, @builtin(local_invocation_index) li: u32) {
    if li < 64u {
        atomicStore(&local_bins[li], 0u);
    }
    workgroupBarrier();
    let size = textureDimensions(picture);
    if all(id.xy < size) {
        let c = textureLoad(picture, id.xy, 0);
        if c.a > 0.0 {
            let l = luma(c.rgb / c.a);
            var bin = 0u;
            if l > exp2(MIN_LOG) {
                let t = clamp((log2(l) - MIN_LOG) / (MAX_LOG - MIN_LOG), 0.0, 1.0);
                bin = 1u + u32(t * 62.0);
            }
            atomicAdd(&local_bins[bin], 1u);
        }
    }
    workgroupBarrier();
    if li < 64u {
        atomicAdd(&state.bins[li], atomicLoad(&local_bins[li]));
    }
}

@compute @workgroup_size(64)
fn cs_adapt(@builtin(local_invocation_index) li: u32) {
    if li == 0u {
        var total = 0u;
        for (var i = 0u; i < 64u; i++) {
            total += atomicLoad(&state.bins[i]);
        }
        let lo = f32(total) * 0.4;
        let hi = f32(total) * 0.95;
        var seen = 0.0;
        var sum = 0.0;
        var count = 0.0;
        for (var i = 0u; i < 64u; i++) {
            let n = f32(atomicLoad(&state.bins[i]));
            // The part of this bin between the two percentiles.
            let take = max(min(seen + n, hi) - max(seen, lo), 0.0);
            var log_l = MIN_LOG;
            if i > 0u {
                log_l = MIN_LOG + (f32(i) - 0.5) / 62.0 * (MAX_LOG - MIN_LOG);
            }
            sum += log_l * take;
            count += take;
            seen += n;
        }
        if count > 0.0 {
            let target_l = exp2(sum / count);
            var l = target_l;
            if state.valid > 0.5 {
                l = state.luminance + (target_l - state.luminance) * post.exposure.y;
            }
            state.luminance = l;
            state.exposure = clamp(0.18 / max(l, 1e-5) * exp2(post.exposure.x), exp2(-12.0), exp2(12.0));
            state.valid = 1.0;
        }
    }
    storageBarrier();
    workgroupBarrier();
    atomicStore(&state.bins[li], 0u);
}
