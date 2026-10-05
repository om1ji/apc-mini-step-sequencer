**English** | [Русский](README.ru.md)

# apc-mini-step-sequencer

A VST3 drum step sequencer built around the AKAI APC mini mk2: the pad grid is the pattern editor, the buttons under it pick the drum track, the faders mix it, and the plugin plays its own samples in sync with the host transport.

Written in Rust with [NIH-plug](https://github.com/robbert-vdh/nih-plug). Developed and tested on macOS (Apple Silicon) in Ableton Live 12. There is a sibling project that does the same thing as a Max for Live device: [apc-mini-step-sequencer-max-for-live](https://github.com/om1ji/apc-mini-step-sequencer-max-for-live).

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/images/architecture-dark.png">
  <img alt="Architecture: the plugin reads pads and faders straight from the APC through midir, the audio thread runs the sequencer against Live's transport and renders samples, the editor feeds decoded samples in, and a background task sends LED colors back to the controller" src="docs/images/architecture-light.png">
</picture>

## How it works

The controller is split like a classic drum machine: the grid shows the pattern of one track at a time.

| Control | Role |
| --- | --- |
| Top two pad rows | 16 steps of one bar, 1/16 each: steps 1–8 on the top row, 9–16 on the row below |
| Buttons under the grid | select the track (sample) shown on the grid, 8 tracks |
| Faders 1–8 | track volume |
| Fader 9 | master volume |
| Right-hand buttons, lower six pad rows | unused |

Pad colors: gray is a step in the pattern, yellow is an active step, green is the step playing now. The button of the selected track is lit. All eight tracks play at once, whichever one is on the grid.

Inside the plugin:

- **MIDI input** (a `midir` callback thread) listens to the APC's Control port. Pad and button presses go into a lock-free ring buffer (`rtrb`), fader positions into atomics.
- **The audio thread** drains presses at the start of every block, reads tempo and position from the host, and checks for a new 1/16 step on every sample, so hits land sample-accurately. It mixes up to 32 voices with linear interpolation, so a sample at any rate plays at the right pitch.
- **The editor** (egui) decodes audio files with `symphonia` and hands finished samples to the audio thread through another ring buffer.
- **A background task** sends the LED frame to the controller, only the pads that changed, and frees replaced samples so the audio thread never deallocates.

Sample paths are saved in the Live set and reloaded when the project opens. The pattern itself is not saved yet — see [Known limitations](#known-limitations).

## Why it is built this way

- **The plugin talks to the controller directly instead of through Live.** Live does not pass MIDI CC to VST3 plugins, so faders never arrived. Pad notes routed through the track proved fragile: they stopped arriving after the plugin was re-added. Reading the port with `midir` works regardless of track routing.
- **The plugin still declares a MIDI input it ignores.** Without one, Live refuses to load an instrument: `plugin has instrument category, but no valid event input bus`. Notes from that input are dropped on purpose; reading them as well would trigger every press twice and cancel each toggle.
- **LED output and sample cleanup run on a background task.** Sending MIDI and freeing memory are not real-time safe; the audio thread only builds a 64-byte frame and queues it.
- **The file dialog opens on its own thread.** Opening it from inside the egui update re-entered the editor while macOS ran the modal loop, and the panic took Live down with it.
- **`egui-baseview` is vendored** in [`vendor/egui-baseview`](vendor/egui-baseview). `baseview` already reports file drag-and-drop on macOS and Windows, but upstream `egui-baseview` dropped those events. The copy forwards them to egui as `hovered_files` / `dropped_files`; `Cargo.toml` swaps it in with `[patch]`.
- **Fader volume is smoothed per sample** (10 ms one-pole) with a squared curve, so fast moves do not click.

## Repository layout

```
src/
  lib.rs          plugin: process loop, transport sync, fader smoothing, background tasks
  sequencer.rs    pattern, grid layout, LED colors
  apc.rs          APC mini mk2 protocol: note/CC numbers, palette, midir in/out
  voice.rs        sample playback, 32 voices
  sample.rs       decoding WAV / AIFF / FLAC / MP3 with symphonia
  editor.rs       egui window: sample slots, drag-and-drop, file dialog
vendor/egui-baseview/   egui-baseview with file drag-and-drop forwarded
xtask/                  NIH-plug bundler (cargo xtask)
bundler.toml            bundle name: "APC Seq"
docs/diagrams/          archify source and interactive diagram
```

## Requirements

- AKAI APC mini mk2.
- A VST3 host. Tested only in Ableton Live 12.0.5 on macOS 26 (Apple Silicon).
- Rust, stable toolchain (built with 1.93). NIH-plug is pulled from git, so the first build needs network access.

## Build and install

```bash
cargo xtask bundle apc_seq --release
```

The bundle lands in `target/bundled/APC Seq.vst3`. On macOS, link it into the user VST3 folder so every rebuild is picked up without copying:

```bash
mkdir -p ~/Library/Audio/Plug-Ins/VST3
ln -sfn "$PWD/target/bundled/APC Seq.vst3" ~/Library/Audio/Plug-Ins/VST3/"APC Seq.vst3"
```

After a rebuild, remove the plugin from the track and add it again; sometimes Live needs a restart.

## Setting up Ableton Live

1. **Settings → Plug-Ins:** turn on **Use VST3 Plug-in System Folders** and rescan.
2. **Settings → Link, Tempo & MIDI:** if **APC mini mk2** is selected as a Control Surface, set that slot to **None**. Live's own script repaints the pads and fights the plugin for the LEDs.
3. Create a MIDI track and drop **Plug-Ins → om1ji → APC Seq** on it. The track's MIDI input does not matter: the plugin reads the controller itself.
4. Open the plugin window and load samples.
5. Press Play in Live and tap the top two pad rows.

## Usage

**Loading samples.** Drag a file from Live's browser onto a track row in the plugin window. The row under the pointer is highlighted while you drag. **Загрузить…** opens a file dialog instead, **▶** auditions the sample. WAV, AIFF, FLAC and MP3 are accepted, mono or stereo, at any sample rate.

**Programming.** Press a button under the grid to pick a track, then toggle steps on the top two rows. Steps can be edited while Live is playing or stopped.

**Mixing.** The controller does not report fader positions until a fader moves, so every fader is assumed to sit at 80 % until you touch it.

The window's text is in Russian. The strings you will see:

| Text | Meaning |
| --- | --- |
| `Контроллер подключён` / `Контроллер не найден` | the APC's Control port is open / was not found |
| `Перетащите сэмпл из браузера Live на строку дорожки` | drop a sample from Live's browser onto a track row |
| `— пусто —` | no sample on this track |
| `Загрузить…` | open a file dialog |

## Testing

```bash
cargo test --lib
```

The tests cover the sequencer: the step ↔ pad layout and toggling steps on the selected track. Timing, audio and the controller are checked by hand in Live.

## Troubleshooting

| Symptom or message | Cause and fix |
| --- | --- |
| Live: `plugin has instrument category, but no valid event input bus` | the build has no MIDI input; `MIDI_INPUT` in `src/lib.rs` must stay `MidiConfig::Basic` |
| Pads flicker or show Live's clip colors | a Control Surface script (APC mini mk2) is driving the LEDs; set that slot to None |
| `Контроллер не найден` in the window | the controller is unplugged or its port has another name; the plugin looks for a port containing `APC mini mk2` that is not `Notes`, `MIDIIN…` or `MIDIOUT…` |
| Faders do nothing until moved, then jump | expected: the starting position is unknown until the fader sends a value |
| `диалог не открылся — перетащите файл` in a row | the file dialog failed to open; drag the file onto the row instead |
| `неизвестный формат`, `кодек не поддерживается`, `в файле нет аудио` | `symphonia` could not decode the file; convert it to WAV |
| Live crashed when pressing **Загрузить…** | builds before the dialog moved to its own thread; rebuild from the current source |

## Known limitations

- **The pattern is not saved** with the Live set. Sample paths are; the steps are empty after reopening the project.
- **One bar, 16 steps, 4/4.** Longer patterns would fit the lower pad rows, but are not implemented.
- **Samples are stored by absolute path.** Moving the files breaks the reference; the slot then stays empty.
- **Windows is untested.** It should build, but the classic Windows MIDI driver lets only one application open a port. If Live also has the APC open, the plugin's `midir` input may fail to connect.
- **Do not run it alongside the Max for Live version** on the same controller — both repaint the pads.

## License

[MIT](LICENSE), for the source code in this repository.

> [!NOTE]
> NIH-plug's VST3 bindings are GPLv3. A compiled `APC Seq.vst3` links them, so anyone distributing the binary must comply with GPLv3. The MIT-licensed source is compatible with that.

[`vendor/egui-baseview`](vendor/egui-baseview) is a modified copy of [egui-baseview](https://github.com/BillyDM/egui-baseview), MIT, with its original [LICENSE](vendor/egui-baseview/LICENSE).

## Diagrams

The interactive diagram is [`docs/diagrams/architecture.html`](docs/diagrams/architecture.html), built from [`docs/diagrams/architecture.json`](docs/diagrams/architecture.json) with [archify](https://github.com/tt-a1i/archify). The PNGs in `docs/images/` are screenshots of it in the light and dark themes. To regenerate the HTML:

```bash
node <path-to-archify>/bin/archify.mjs deliver architecture \
  docs/diagrams/architecture.json docs/diagrams/architecture.html --quality showcase
```
