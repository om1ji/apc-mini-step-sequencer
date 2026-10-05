mod apc;
mod editor;
mod sample;
mod sequencer;
mod voice;

use apc::{Controller, Faders, Frame, LedTask, FADER_COUNT, PAD_COUNT, TRACK_BUTTON_FIRST};
use nih_plug::prelude::*;
use nih_plug_egui::EguiState;
use sample::Sample;
use sequencer::{Sequencer, STEPS, TRACKS};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use voice::{Slots, Voices};

/// Индекс мастер-фейдера (девятый, самый правый).
const MASTER: usize = FADER_COUNT - 1;
/// Положение фейдеров до первого движения: контроллер не сообщает, где они стоят.
const FADER_DEFAULT: f32 = 0.8;
/// За сколько секунд громкость догоняет фейдер — без щелчков при резком движении.
const FADER_SMOOTHING_SECS: f32 = 0.01;

struct ApcSeq {
    params: Arc<ApcSeqParams>,
    sample_rate: f32,
    voices: Voices,
    sequencer: Sequencer,
    /// Загруженные сэмплы дорожек. Меняются только в аудио-потоке.
    slots: Slots,
    /// Сюда окно присылает новые сэмплы.
    loads: rtrb::Consumer<(usize, Arc<Sample>)>,
    shared: Arc<Shared>,
    /// Номер текущей 1/16 от начала аранжировки; `None`, когда Live стоит.
    sixteenth: Option<i64>,
    /// Подсветку нужно обновить в конце блока.
    redraw: bool,
    /// Положения фейдеров (их пишет MIDI-поток) и текущая сглаженная громкость.
    faders: Arc<Faders>,
    /// Нажатия падов и кнопок с контроллера.
    presses: rtrb::Consumer<u8>,
    fader_gain: [f32; FADER_COUNT],
    fader_coef: f32,
}

/// Общее между окном плагина и аудио-потоком.
pub struct Shared {
    loads: Mutex<rtrb::Producer<(usize, Arc<Sample>)>>,
    /// «Прослушать сэмпл»: окно ставит флаг, аудио-поток играет и сбрасывает.
    preview: [AtomicBool; TRACKS],
    controller: Mutex<Controller>,
    /// Ошибка последней загрузки по дорожке — показывается в окне.
    load_errors: Mutex<[Option<String>; TRACKS]>,
    /// Открыт ли диалог выбора файла — второй не открываем.
    dialog_open: AtomicBool,
}

/// Параметров для автоматизации пока нет: громкости живут на фейдерах.
/// Поля `persist` Live сохраняет в проекте.
#[derive(Params)]
pub struct ApcSeqParams {
    #[persist = "editor-state"]
    editor_state: Arc<EguiState>,
    #[persist = "sample-paths"]
    sample_paths: Mutex<[Option<String>; TRACKS]>,
}

impl Default for ApcSeqParams {
    fn default() -> Self {
        Self { editor_state: EguiState::from_size(460, 380), sample_paths: Mutex::default() }
    }
}

/// Фоновые задачи: всё, что нельзя делать в аудио-потоке.
pub enum Task {
    Leds(LedTask),
    /// Старый сэмпл после замены — освобождаем память не в аудио-потоке.
    DropSample(Arc<Sample>),
}

impl Default for ApcSeq {
    fn default() -> Self {
        let (producer, consumer) = rtrb::RingBuffer::new(16);
        let faders = apc::new_faders(FADER_DEFAULT);
        let (press_producer, press_consumer) = rtrb::RingBuffer::new(256);
        Self {
            params: Arc::new(ApcSeqParams::default()),
            sample_rate: 44100.0,
            voices: Voices::default(),
            sequencer: Sequencer::new(),
            slots: Default::default(),
            loads: consumer,
            shared: Arc::new(Shared {
                loads: Mutex::new(producer),
                preview: Default::default(),
                controller: Mutex::new(Controller::new(faders.clone(), Arc::new(Mutex::new(press_producer)))),
                load_errors: Mutex::default(),
                dialog_open: AtomicBool::new(false),
            }),
            sixteenth: None,
            redraw: true,
            faders,
            presses: press_consumer,
            fader_gain: [fader_to_gain(FADER_DEFAULT); FADER_COUNT],
            fader_coef: 1.0,
        }
    }
}

/// Квадратичная кривая: ход фейдера ближе к тому, как громкость слышит ухо.
fn fader_to_gain(value: f32) -> f32 {
    value * value
}

impl ApcSeq {
    fn current_step(&self) -> Option<usize> {
        self.sixteenth.map(|n| n.rem_euclid(STEPS as i64) as usize)
    }

    /// Разбирает нажатия, накопившиеся с прошлого блока.
    fn take_presses(&mut self) {
        while let Ok(note) = self.presses.pop() {
            if (note as usize) < PAD_COUNT {
                let pad = note as usize;
                self.sequencer.on_pad(pad / 8, pad % 8);
                self.redraw = true;
            } else if (TRACK_BUTTON_FIRST..TRACK_BUTTON_FIRST + TRACKS as u8).contains(&note) {
                self.sequencer.on_track_button((note - TRACK_BUTTON_FIRST) as usize);
                self.redraw = true;
            }
        }
    }

    /// Принимает сэмплы из окна и запросы на прослушивание. Зовётся в начале блока.
    fn take_gui_requests(&mut self, context: &mut impl ProcessContext<Self>) {
        while let Ok((track, sample)) = self.loads.pop() {
            self.voices.stop_track(track);
            if let Some(old) = self.slots[track].replace(sample) {
                context.execute_background(Task::DropSample(old));
            }
        }
        for track in 0..TRACKS {
            if self.shared.preview[track].swap(false, Ordering::Relaxed) {
                self.voices.trigger(track);
            }
        }
    }

    /// После открытия проекта подгружает сэмплы по сохранённым путям.
    fn load_saved_samples(&mut self) {
        let paths = self.params.sample_paths.lock().unwrap().clone();
        for (slot, path) in self.slots.iter_mut().zip(paths) {
            if slot.as_ref().map(|s| &s.path) != path.as_ref() {
                *slot = path.and_then(|p| sample::load(&p).ok()).map(Arc::new);
            }
        }
    }

    fn next_track_gains(&mut self, fader_target: &[f32; FADER_COUNT]) -> [f32; TRACKS] {
        for (gain, &target) in self.fader_gain.iter_mut().zip(fader_target) {
            *gain += (fader_to_gain(target) - *gain) * self.fader_coef;
        }
        let master = self.fader_gain[MASTER];
        std::array::from_fn(|track| self.fader_gain[track] * master)
    }
}

impl Plugin for ApcSeq {
    const NAME: &'static str = "APC Seq";
    const VENDOR: &'static str = "om1ji";
    const URL: &'static str = "";
    const EMAIL: &'static str = "";
    const VERSION: &'static str = env!("CARGO_PKG_VERSION");

    const AUDIO_IO_LAYOUTS: &'static [AudioIOLayout] = &[AudioIOLayout {
        main_input_channels: None,
        main_output_channels: NonZeroU32::new(2),
        ..AudioIOLayout::const_default()
    }];

    /// Live не загружает инструмент без MIDI-входа, поэтому вход объявлен.
    /// Но ноты из него не читаем: контроллер плагин слушает сам, иначе каждое
    /// нажатие срабатывало бы дважды.
    const MIDI_INPUT: MidiConfig = MidiConfig::Basic;
    const SAMPLE_ACCURATE_AUTOMATION: bool = true;

    type SysExMessage = ();
    type BackgroundTask = Task;

    fn params(&self) -> Arc<dyn Params> {
        self.params.clone()
    }

    fn task_executor(&mut self) -> TaskExecutor<Self> {
        let shared = self.shared.clone();
        Box::new(move |task| match task {
            Task::Leds(task) => shared.controller.lock().unwrap().run(task),
            Task::DropSample(sample) => drop(sample),
        })
    }

    fn editor(&mut self, _async_executor: AsyncExecutor<Self>) -> Option<Box<dyn Editor>> {
        editor::create(self.params.clone(), self.shared.clone())
    }

    fn initialize(
        &mut self,
        _layout: &AudioIOLayout,
        buffer_config: &BufferConfig,
        _context: &mut impl InitContext<Self>,
    ) -> bool {
        self.sample_rate = buffer_config.sample_rate;
        self.fader_coef = 1.0 - (-1.0 / (FADER_SMOOTHING_SECS * self.sample_rate)).exp();
        self.shared.controller.lock().unwrap().run(LedTask::ClearAll);
        self.load_saved_samples();
        self.sixteenth = None;
        self.redraw = true;
        true
    }

    fn deactivate(&mut self) {
        self.shared.controller.lock().unwrap().run(LedTask::ClearAll);
    }

    fn process(
        &mut self,
        buffer: &mut Buffer,
        _aux: &mut AuxiliaryBuffers,
        context: &mut impl ProcessContext<Self>,
    ) -> ProcessStatus {
        self.take_gui_requests(context);
        self.take_presses();

        let transport = context.transport();
        let playing = transport.playing;
        let block_start = transport.pos_beats();
        let beats_per_sample = transport.tempo.map(|bpm| bpm / 60.0 / self.sample_rate as f64);

        // Фейдеры читаем раз за блок: несколько миллисекунд задержки не слышно,
        // а сглаживание всё равно идёт по каждому сэмплу.
        let fader_target: [f32; FADER_COUNT] = std::array::from_fn(|i| apc::read_fader(&self.faders, i));

        for (sample_id, channel_samples) in buffer.iter_samples().enumerate() {
            // Позиция в 1/16 на этом сэмпле: 1 доля = 4 шестнадцатых.
            let sixteenth = match (playing, block_start, beats_per_sample) {
                (true, Some(start), Some(step)) => {
                    Some(((start + sample_id as f64 * step) * 4.0).floor() as i64)
                }
                _ => None,
            };
            // Сравниваем на неравенство, а не «+1»: так работают и луп, и перемотка.
            if sixteenth != self.sixteenth {
                self.sixteenth = sixteenth;
                self.redraw = true;
                if let Some(step) = self.current_step() {
                    let hits = self.sequencer.on_step(step);
                    for track in (0..TRACKS).filter(|&t| hits[t]) {
                        self.voices.trigger(track);
                    }
                }
            }

            let gains = self.next_track_gains(&fader_target);
            let out = self.voices.next_frame(self.sample_rate, &self.slots, &gains);
            for (sample, value) in channel_samples.into_iter().zip(out) {
                *sample = value;
            }
        }

        if self.redraw {
            self.redraw = false;
            let mut frame = Frame::BLANK;
            self.sequencer.render(self.current_step(), &mut frame);
            context.execute_background(Task::Leds(LedTask::Show(frame)));
        }

        ProcessStatus::Normal
    }
}

impl Vst3Plugin for ApcSeq {
    const VST3_CLASS_ID: [u8; 16] = *b"om1jiApcSeqDrums";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] =
        &[Vst3SubCategory::Instrument, Vst3SubCategory::Drum];
}

nih_export_vst3!(ApcSeq);
