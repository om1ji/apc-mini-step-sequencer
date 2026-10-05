//! Окно плагина: слоты сэмплов по дорожкам.
//!
//! Окно живёт в своём потоке. С аудио-потоком оно общается только через
//! `Shared`: готовые сэмплы уходят в очередь без блокировок, прослушивание —
//! через атомарные флаги.

use std::sync::atomic::Ordering;
use std::sync::Arc;

use nih_plug::prelude::Editor;
use nih_plug_egui::{create_egui_editor, egui};

use crate::sample;
use crate::sequencer::TRACKS;
use crate::{ApcSeqParams, Shared};

const AUDIO_EXTENSIONS: &[&str] = &["wav", "aif", "aiff", "flac", "mp3"];

pub fn create(params: Arc<ApcSeqParams>, shared: Arc<Shared>) -> Option<Box<dyn Editor>> {
    create_egui_editor(
        params.editor_state.clone(),
        (),
        |_, _| {},
        move |ctx, _setter, _state| {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.heading("APC Seq");
                let connected = shared.controller.lock().is_ok_and(|c| c.connected());
                ui.label(if connected { "Контроллер подключён" } else { "Контроллер не найден" });
                ui.separator();

                ui.weak("Перетащите сэмпл из браузера Live на строку дорожки");
                ui.add_space(4.0);
                for track in 0..TRACKS {
                    track_row(ui, track, &params, &shared);
                }
            });
        },
    )
}

fn track_row(ui: &mut egui::Ui, track: usize, params: &Arc<ApcSeqParams>, shared: &Arc<Shared>) {
    let path = params.sample_paths.lock().unwrap()[track].clone();
    let error = shared.load_errors.lock().unwrap()[track].clone();

    // Файл тащат над этой строкой — подсвечиваем её.
    let row_rect = egui::Rect::from_min_size(ui.cursor().min, egui::vec2(ui.available_width(), 30.0));
    let (dragging, dropped, pointer) = ui.input(|i| {
        let dropped = i.raw.dropped_files.first().and_then(|f| f.path.clone());
        (!i.raw.hovered_files.is_empty(), dropped, i.pointer.hover_pos())
    });
    let under_pointer = pointer.is_some_and(|p| row_rect.contains(p));
    if dragging && under_pointer {
        ui.painter().rect_filled(row_rect, 4.0, ui.visuals().selection.bg_fill);
    }
    if let (Some(dropped), true) = (dropped, under_pointer) {
        let result = load_into(track, &dropped.to_string_lossy(), params, shared);
        shared.load_errors.lock().unwrap()[track] = result.err();
    }

    ui.allocate_ui_with_layout(row_rect.size(), egui::Layout::left_to_right(egui::Align::Center), |ui| {
        ui.add_sized([20.0, 20.0], egui::Label::new(format!("{}", track + 1)));

        let dialog_open = shared.dialog_open.load(Ordering::Relaxed);
        if ui.add_enabled(!dialog_open, egui::Button::new("Загрузить…")).clicked() {
            pick_and_load(track, params.clone(), shared.clone(), ui.ctx().clone());
        }
        if ui.add_enabled(path.is_some(), egui::Button::new("▶")).clicked() {
            shared.preview[track].store(true, Ordering::Relaxed);
        }

        match (error, &path) {
            (Some(err), _) => ui.colored_label(egui::Color32::LIGHT_RED, err),
            (None, Some(path)) => ui.label(sample::file_name(path)),
            (None, None) => ui.weak("— пусто —"),
        };
    });
}

/// Диалог выбора файла открываем в отдельном потоке. Если открыть его прямо
/// из отрисовки окна, macOS во время диалога снова вызовет отрисовку изнутри
/// неё же — и плагин вместе с Live падает.
fn pick_and_load(track: usize, params: Arc<ApcSeqParams>, shared: Arc<Shared>, ctx: egui::Context) {
    shared.dialog_open.store(true, Ordering::Relaxed);
    std::thread::spawn(move || {
        // Паника здесь роняет только этот поток, но флаг диалога надо снять в любом случае.
        let picked = std::panic::catch_unwind(|| {
            pollster::block_on(rfd::AsyncFileDialog::new().add_filter("Аудио", AUDIO_EXTENSIONS).pick_file())
        });
        match picked {
            Ok(Some(file)) => {
                let result = load_into(track, &file.path().to_string_lossy(), &params, &shared);
                shared.load_errors.lock().unwrap()[track] = result.err();
            }
            Ok(None) => (),
            Err(_) => {
                shared.load_errors.lock().unwrap()[track] =
                    Some("диалог не открылся — перетащите файл".to_owned());
            }
        }
        shared.dialog_open.store(false, Ordering::Relaxed);
        ctx.request_repaint();
    });
}

/// Декодирует файл здесь, вне аудио-потока, и отдаёт ему готовый сэмпл.
fn load_into(track: usize, path: &str, params: &ApcSeqParams, shared: &Shared) -> Result<(), String> {
    let sample = sample::load(path)?;
    shared
        .loads
        .lock()
        .unwrap()
        .push((track, Arc::new(sample)))
        .map_err(|_| "очередь занята, попробуйте ещё раз".to_owned())?;
    params.sample_paths.lock().unwrap()[track] = Some(path.to_owned());
    Ok(())
}
