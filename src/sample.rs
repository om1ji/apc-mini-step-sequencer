//! Загрузка аудиофайлов в память.

use std::path::Path;

use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::DecoderOptions;
use symphonia::core::errors::Error;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

/// Сэмпл целиком в памяти, стерео. Моно-файлы раскладываются на оба канала.
pub struct Sample {
    pub path: String,
    pub frames: Vec<[f32; 2]>,
    pub sample_rate: f32,
}

/// Читает WAV/AIFF/FLAC/MP3. Медленно для аудио-потока — зовём только из GUI
/// или `initialize`.
pub fn load(path: &str) -> Result<Sample, String> {
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let stream = MediaSourceStream::new(Box::new(file), Default::default());

    let mut hint = Hint::new();
    if let Some(ext) = Path::new(path).extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }
    let mut format = symphonia::default::get_probe()
        .format(&hint, stream, &FormatOptions::default(), &MetadataOptions::default())
        .map_err(|e| format!("неизвестный формат: {e}"))?
        .format;

    let track = format.default_track().ok_or("в файле нет аудио")?;
    let track_id = track.id;
    let sample_rate = track.codec_params.sample_rate.ok_or("не указана частота")? as f32;
    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .map_err(|e| format!("кодек не поддерживается: {e}"))?;

    let mut frames = Vec::new();
    loop {
        let packet = match format.next_packet() {
            Ok(packet) => packet,
            Err(Error::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(e.to_string()),
        };
        if packet.track_id() != track_id {
            continue;
        }
        let decoded = decoder.decode(&packet).map_err(|e| e.to_string())?;
        let spec = *decoded.spec();
        let channels = spec.channels.count();
        let mut buf = SampleBuffer::<f32>::new(decoded.capacity() as u64, spec);
        buf.copy_interleaved_ref(decoded);
        frames.extend(buf.samples().chunks(channels).map(|f| match f {
            [mono] => [*mono, *mono],
            [l, r, ..] => [*l, *r],
            [] => [0.0, 0.0],
        }));
    }

    Ok(Sample { path: path.to_owned(), frames, sample_rate })
}

/// Имя файла без папки — для интерфейса.
pub fn file_name(path: &str) -> &str {
    Path::new(path).file_name().and_then(|n| n.to_str()).unwrap_or(path)
}

