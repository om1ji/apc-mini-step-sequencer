//! Логика секвенсора.
//!
//! Плагин сам вызывает методы:
//!   * `on_pad`          — нажали пад сетки;
//!   * `on_track_button` — нажали кнопку под сеткой (выбор дорожки);
//!   * `on_step`         — транспорт Live дошёл до очередной 1/16;
//!   * `render`          — нужно обновить подсветку (после нажатия или смены шага).
//!
//! Раскладка: на сетке паттерн выбранной дорожки. 16 шагов занимают два верхних
//! ряда: шаги 1–8 — верхний ряд, 9–16 — второй сверху. Остальные ряды пока пустые.
//!
//! Координаты пада: `row` 0 — НИЖНИЙ ряд, 7 — верхний; `col` 0 — левая колонка.
//! В кадре индекс пада = row * 8 + col.
//!
//! Эти методы вызываются из аудио-потока: никаких выделений памяти, блокировок
//! и паник — только фиксированные массивы.

use crate::apc::{color, Frame};

pub const TRACKS: usize = 8;
pub const STEPS: usize = 16;

const COLS: usize = 8;
/// Верхний ряд сетки — с него начинается паттерн.
const TOP_ROW: usize = 7;

pub struct Sequencer {
    /// `pattern[track][step]` — включён ли шаг.
    pattern: [[bool; STEPS]; TRACKS],
    /// Дорожка, которая показана на сетке и редактируется.
    selected: usize,
}

/// Шаг → пад: шаги идут по рядам сверху вниз, слева направо.
fn step_to_pad(step: usize) -> usize {
    let row = TOP_ROW - step / COLS;
    let col = step % COLS;
    row * COLS + col
}

/// Пад → шаг, если пад относится к паттерну.
fn pad_to_step(row: usize, col: usize) -> Option<usize> {
    let step = (TOP_ROW.checked_sub(row)?) * COLS + col;
    (step < STEPS).then_some(step)
}

impl Sequencer {
    pub fn new() -> Self {
        Self { pattern: [[false; STEPS]; TRACKS], selected: 0 }
    }

    /// Нажатие пада: переключает шаг выбранной дорожки.
    pub fn on_pad(&mut self, row: usize, col: usize) {
        if let Some(step) = pad_to_step(row, col) {
            let cell = &mut self.pattern[self.selected][step];
            *cell = !*cell;
        }
    }

    /// Нажатие кнопки под сеткой: `track` 0..8 слева направо.
    pub fn on_track_button(&mut self, track: usize) {
        self.selected = track;
    }

    /// Какие дорожки звучат на шаге `step`. Играют все, а не только выбранная.
    pub fn on_step(&mut self, step: usize) -> [bool; TRACKS] {
        std::array::from_fn(|track| self.pattern[track][step])
    }

    /// Серые — шаги паттерна, жёлтые — включённые, зелёный — текущий шаг.
    /// Под нижней сеткой горит кнопка выбранной дорожки.
    pub fn render(&self, current_step: Option<usize>, frame: &mut Frame) {
        let pattern = &self.pattern[self.selected];
        for step in 0..STEPS {
            frame.pads[step_to_pad(step)] = if current_step == Some(step) {
                color::GREEN
            } else if pattern[step] {
                color::YELLOW
            } else {
                color::GRAY
            };
        }
        frame.track_buttons[self.selected] = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_round_trip() {
        // Шаг 1 — левый верхний пад, шаг 9 — первая колонка второго ряда сверху.
        assert_eq!(step_to_pad(0), 56);
        assert_eq!(step_to_pad(8), 48);
        assert_eq!(step_to_pad(15), 55);
        for step in 0..STEPS {
            let pad = step_to_pad(step);
            assert_eq!(pad_to_step(pad / COLS, pad % COLS), Some(step));
        }
        // Нижние шесть рядов к паттерну не относятся.
        assert_eq!(pad_to_step(5, 0), None);
        assert_eq!(pad_to_step(0, 7), None);
    }

    #[test]
    fn toggles_and_plays_selected_track() {
        let mut seq = Sequencer::new();
        seq.on_track_button(2);
        seq.on_pad(7, 0); // шаг 0
        seq.on_pad(6, 4); // шаг 12
        assert_eq!(seq.on_step(0), [false, false, true, false, false, false, false, false]);
        assert!(seq.on_step(12)[2]);
        seq.on_pad(6, 4);
        assert!(!seq.on_step(12)[2]);
    }
}
