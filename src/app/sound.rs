//! The themes' tunes on bells (Christmas' « Jingle Bells », Gryffindor's opening): the system's sound
//! output, opened when first played.

pub(super) struct Sound {
    _device: rodio::MixerDeviceSink,
    mixer: rodio::mixer::Mixer,
}

impl Sound {
    pub(super) fn open() -> Option<Self> {
        let mut device = rodio::DeviceSinkBuilder::open_default_sink()
            .map_err(|e| crate::log::error(&format!("sound: {e}")))
            .ok()?;
        device.log_on_drop(false);
        let mixer = device.mixer().clone();
        Some(Self {
            _device: device,
            mixer,
        })
    }

    /// A tune on bells (a celesta): each note's MIDI number (0: a rest) and length; each one struck
    /// (its octave and twelfth ringing with it) and left to ring on under the next.
    pub(super) fn bells(&self, tune: &[(f32, f32)], loud: f32) {
        const RATE: u32 = 44_100;
        const RING: f32 = 1.4;
        let total: f32 = tune.iter().map(|(_, len)| len).sum::<f32>() + RING;
        let mut samples = vec![0.0f32; (total * RATE as f32) as usize];
        let mut at = 0.0f32;
        for &(note, len) in tune {
            if note > 0.0 {
                let f = 440.0 * 2f32.powf((note - 69.0) / 12.0);
                let start = (at * RATE as f32) as usize;
                let n = (RING * RATE as f32) as usize;
                for k in 0..n {
                    let Some(out) = samples.get_mut(start + k) else {
                        break;
                    };
                    let t = k as f32 / RATE as f32;
                    let env = (t * 400.0).min(1.0) * (-t * 3.2).exp();
                    let tau = std::f32::consts::TAU;
                    let wave = (tau * f * t).sin()
                        + 0.35 * (tau * 2.0 * f * t).sin() * (-t * 6.0).exp()
                        + 0.12 * (tau * 3.0 * f * t).sin() * (-t * 9.0).exp();
                    *out += wave * env * loud * 0.2;
                }
            }
            at += len;
        }
        let (Some(channels), Some(rate)) =
            (std::num::NonZero::new(1), std::num::NonZero::new(RATE))
        else {
            return;
        };
        self.mixer
            .add(rodio::buffer::SamplesBuffer::new(channels, rate, samples));
    }
}
