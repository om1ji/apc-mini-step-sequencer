//! Протокол APC mini mk2: раскладка элементов и отправка подсветки на контроллер.
//!
//! Плагин работает с контроллером напрямую, мимо Live: сам читает нажатия
//! и фейдеры и сам шлёт подсветку. Live не передаёт CC в VST3-плагины, а маршрут
//! нот через дорожку легко сбить — поэтому MIDI от Live плагин не принимает вовсе.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use midir::{MidiInput, MidiInputConnection, MidiOutput, MidiOutputConnection};

/// Пады сетки — ноты 0..=63. Нота 0 — левый нижний пад, 63 — правый верхний.
pub const PAD_COUNT: usize = 64;
/// Нижний ряд кнопок под сеткой — ноты 100..=107.
pub const TRACK_BUTTON_FIRST: u8 = 100;
/// Правый столбец кнопок — ноты 112..=119 (пока не используются).
#[allow(dead_code)]
pub const SCENE_BUTTON_FIRST: u8 = 112;
/// Фейдеры — CC 48..=56, слева направо. Девятый (CC 56) — мастер.
pub const FADER_CC_FIRST: u8 = 48;
pub const FADER_COUNT: usize = 9;

/// Положения фейдеров 0..1 (биты f32). Пишет MIDI-поток, читает аудио-поток —
/// атомики, чтобы без блокировок.
pub type Faders = [AtomicU32; FADER_COUNT];

pub fn new_faders(initial: f32) -> Arc<Faders> {
    Arc::new(std::array::from_fn(|_| AtomicU32::new(initial.to_bits())))
}

pub fn read_fader(faders: &Faders, i: usize) -> f32 {
    f32::from_bits(faders[i].load(Ordering::Relaxed))
}

/// Индексы встроенной палитры APC mini mk2 (velocity в Note On).
#[allow(dead_code)]
pub mod color {
    pub const OFF: u8 = 0;
    pub const DARK_GRAY: u8 = 1;
    pub const GRAY: u8 = 2;
    pub const WHITE: u8 = 3;
    pub const RED: u8 = 5;
    pub const ORANGE: u8 = 9;
    pub const YELLOW: u8 = 13;
    pub const GREEN: u8 = 21;
    pub const CYAN: u8 = 37;
    pub const BLUE: u8 = 45;
    pub const PURPLE: u8 = 49;
    pub const PINK: u8 = 57;
}

/// Всё, что горит на контроллере.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Frame {
    /// Цвет каждого пада из палитры `color`; индекс = row * 8 + col.
    pub pads: [u8; PAD_COUNT],
    /// Нижние кнопки одноцветные (красные): горит или нет.
    pub track_buttons: [bool; 8],
}

impl Frame {
    pub const BLANK: Frame = Frame { pads: [color::OFF; PAD_COUNT], track_buttons: [false; 8] };
}

/// Note On на канале 7 — пад горит на 100% яркости цветом из палитры.
const NOTE_ON_FULL_BRIGHTNESS: u8 = 0x96;
/// Note On на канале 1 — для одноцветных кнопок: 0 — выкл, 1 — вкл.
const NOTE_ON: u8 = 0x90;

/// Очередь нажатий (номера нот) из MIDI-потока в аудио-поток.
pub type Presses = Arc<Mutex<rtrb::Producer<u8>>>;

/// Задача для фонового потока: MIDI-вывод не делаем из аудио-потока.
#[derive(Debug, Clone, Copy)]
pub enum LedTask {
    Show(Frame),
    ClearAll,
}

/// Подключение к портам контроллера: выход — подсветка, вход — нажатия и фейдеры.
/// Открывается лениво при первой задаче, поэтому плагин загружается,
/// даже если контроллер не подключён.
pub struct Controller {
    conn: Option<MidiOutputConnection>,
    input: Option<MidiInputConnection<()>>,
    faders: Arc<Faders>,
    presses: Presses,
    /// Что сейчас горит на контроллере; `None` — неизвестно, слать всё.
    shown: Option<Frame>,
}

impl Controller {
    pub fn new(faders: Arc<Faders>, presses: Presses) -> Self {
        Self { conn: None, input: None, faders, presses, shown: None }
    }

    pub fn connected(&self) -> bool {
        self.conn.is_some()
    }

    pub fn run(&mut self, task: LedTask) {
        if self.conn.is_none() {
            self.conn = connect_output();
            self.input = connect_input(self.faders.clone(), self.presses.clone());
            self.shown = None;
        }
        let Some(conn) = self.conn.as_mut() else {
            return;
        };

        let frame = match task {
            LedTask::Show(frame) => frame,
            LedTask::ClearAll => Frame::BLANK,
        };
        let shown = self.shown;

        // Шлём только то, что изменилось.
        let mut ok = true;
        for (pad, &c) in frame.pads.iter().enumerate() {
            if shown.is_none_or(|s| s.pads[pad] != c) {
                ok &= conn.send(&[NOTE_ON_FULL_BRIGHTNESS, pad as u8, c]).is_ok();
            }
        }
        for (i, &on) in frame.track_buttons.iter().enumerate() {
            if shown.is_none_or(|s| s.track_buttons[i] != on) {
                ok &= conn.send(&[NOTE_ON, TRACK_BUTTON_FIRST + i as u8, on as u8]).is_ok();
            }
        }

        if ok {
            self.shown = Some(frame);
        } else {
            // Контроллер отключили — переподключимся при следующей задаче.
            self.conn = None;
            self.input = None;
        }
    }
}

/// Порт Control: на macOS он называется «APC mini mk2 Control», на Windows —
/// «APC mini mk2» (второй порт там «MIDIIN2/MIDIOUT2 (APC mini mk2)»).
fn is_control_port(name: &str) -> bool {
    name.contains("APC mini mk2")
        && !name.contains("Notes")
        && !name.starts_with("MIDIIN")
        && !name.starts_with("MIDIOUT")
}

fn connect_output() -> Option<MidiOutputConnection> {
    let out = MidiOutput::new("APC Seq").ok()?;
    let port = out.ports().into_iter().find(|p| out.port_name(p).is_ok_and(|n| is_control_port(&n)))?;
    out.connect(&port, "APC Seq LEDs").ok()
}

/// Слушает нажатия и фейдеры. На macOS порт можно делить с Live. На Windows классический
/// драйвер даёт открыть вход только одной программе — если Live уже занял его,
/// подключение не удастся.
fn connect_input(faders: Arc<Faders>, presses: Presses) -> Option<MidiInputConnection<()>> {
    let input = MidiInput::new("APC Seq").ok()?;
    let port = input.ports().into_iter().find(|p| input.port_name(p).is_ok_and(|n| is_control_port(&n)))?;
    input
        .connect(
            &port,
            "APC Seq input",
            move |_, msg, _| match *msg {
                // Нажатие пада или кнопки: Note On на канале 1. Отпускание —
                // Note On с velocity 0 или Note Off, их пропускаем.
                [0x90, note, velocity] if velocity > 0 => {
                    // Если аудио-поток не успевает разбирать очередь — нажатие теряется.
                    let _ = presses.lock().unwrap().push(note);
                }
                // Фейдер: Control Change на канале 1, [0xB0, номер CC, значение 0..127].
                [0xB0, cc, value] if (FADER_CC_FIRST..FADER_CC_FIRST + FADER_COUNT as u8).contains(&cc) => {
                    let v = value as f32 / 127.0;
                    faders[(cc - FADER_CC_FIRST) as usize].store(v.to_bits(), Ordering::Relaxed);
                }
                _ => (),
            },
            (),
        )
        .ok()
}
