//! Воспроизведение сэмплов дорожек.

use std::sync::Arc;

use crate::sample::Sample;
use crate::sequencer::TRACKS;

const MAX_VOICES: usize = 32;

pub type Slots = [Option<Arc<Sample>>; TRACKS];

#[derive(Clone, Copy, Default)]
struct Voice {
    active: bool,
    track: usize,
    /// Позиция в кадрах сэмпла; дробная, потому что частоты файла и Live могут не совпадать.
    pos: f64,
}

pub struct Voices {
    voices: [Voice; MAX_VOICES],
    next: usize,
}

impl Default for Voices {
    fn default() -> Self {
        Self { voices: [Voice::default(); MAX_VOICES], next: 0 }
    }
}

impl Voices {
    /// Запускает сэмпл дорожки. Если все голоса заняты — перезаписывает самый старый.
    pub fn trigger(&mut self, track: usize) {
        self.voices[self.next] = Voice { active: true, track, pos: 0.0 };
        self.next = (self.next + 1) % MAX_VOICES;
    }

    /// Обрывает звучащие голоса дорожки — перед заменой её сэмпла.
    pub fn stop_track(&mut self, track: usize) {
        for v in self.voices.iter_mut().filter(|v| v.track == track) {
            v.active = false;
        }
    }

    /// Громкость дорожки применяется на каждом сэмпле — фейдер слышно сразу,
    /// даже на уже звучащем ударе.
    pub fn next_frame(&mut self, sample_rate: f32, slots: &Slots, track_gains: &[f32; TRACKS]) -> [f32; 2] {
        let mut out = [0.0; 2];
        for v in self.voices.iter_mut().filter(|v| v.active) {
            let Some(sample) = &slots[v.track] else {
                v.active = false;
                continue;
            };
            let i = v.pos as usize;
            if i + 1 >= sample.frames.len() {
                v.active = false;
                continue;
            }
            // Линейная интерполяция между соседними кадрами.
            let t = (v.pos - i as f64) as f32;
            let (a, b) = (sample.frames[i], sample.frames[i + 1]);
            let gain = track_gains[v.track];
            out[0] += (a[0] + (b[0] - a[0]) * t) * gain;
            out[1] += (a[1] + (b[1] - a[1]) * t) * gain;

            v.pos += (sample.sample_rate / sample_rate) as f64;
        }
        out
    }
}
