**English** | [Русский](ARCHITECTURE.ru.md)

# How APC Seq works

This document is for anyone who wants to change the plugin. For installing and playing it, see the [README](../README.md).

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="images/architecture-dark.png">
  <img alt="Architecture: the plugin reads pads and faders straight from the APC through midir, the audio thread runs the sequencer against Live's transport and renders samples, the editor feeds decoded samples in, and a background task sends LED colors back to the controller" src="images/architecture-light.png">
</picture>

## Threads and data flow

- **MIDI input** (a `midir` callback thread) listens to the APC's Control port. Pad and button presses go into a lock-free ring buffer (`rtrb`), fader positions into atomics.
- **The audio thread** drains presses at the start of every block, reads tempo and position from the host, and checks for a new 1/16 step on every sample, so hits land sample-accurately. It mixes up to 32 voices with linear interpolation, so a sample at any rate plays at the right pitch.
- **The editor** (egui) decodes audio files with `symphonia` and hands finished samples to the audio thread through another ring buffer.
- **A background task** sends the LED frame to the controller, only the pads that changed, and frees replaced samples so the audio thread never deallocates.

Sample paths are saved in the host project through NIH-plug's `#[persist]` fields and reloaded in `initialize`. The pattern lives only in the audio thread's `Sequencer` and is not persisted.

## Why it is built this way

- **The plugin talks to the controller directly instead of through Live.** Live does not pass MIDI CC to VST3 plugins, so faders never arrived. Pad notes routed through the track proved fragile: they stopped arriving after the plugin was re-added. Reading the port with `midir` works regardless of track routing.
- **The plugin still declares a MIDI input it ignores.** Without one, Live refuses to load an instrument: `plugin has instrument category, but no valid event input bus`. Notes from that input are dropped on purpose; reading them as well would trigger every press twice and cancel each toggle.
- **LED output and sample cleanup run on a background task.** Sending MIDI and freeing memory are not real-time safe; the audio thread only builds a 64-byte frame and queues it.
- **The file dialog opens on its own thread.** Opening it from inside the egui update re-entered the editor while macOS ran the modal loop, and the panic took Live down with it.
- **`egui-baseview` is vendored** in [`vendor/egui-baseview`](../vendor/egui-baseview). `baseview` already reports file drag-and-drop on macOS and Windows, but upstream `egui-baseview` dropped those events. The copy forwards them to egui as `hovered_files` / `dropped_files`; `Cargo.toml` swaps it in with `[patch]`.
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

## Controller protocol

| Element | MIDI |
| --- | --- |
| Pads | notes 0–63 on channel 1; note 0 is bottom-left, 63 top-right |
| Buttons under the grid | notes 100–107 |
| Right-hand buttons | notes 112–119 |
| Faders | CC 48–56 on channel 1 |
| Pad color | Note On to the pad's note on channel 7 (`0x96`, full brightness), velocity = palette index |
| Button LED | Note On on channel 1, velocity 0 off, 1 on |

The plugin opens the port whose name contains `APC mini mk2` and neither contains `Notes` nor starts with `MIDIIN` or `MIDIOUT`: on macOS that is `APC mini mk2 Control`, on Windows the first of the controller's two ports.

## Testing

```bash
cargo test --lib
```

The tests cover the sequencer: the step ↔ pad layout and toggling steps on the selected track. Timing, audio and the controller are checked by hand in Live.

## Diagrams

The interactive diagram is [`diagrams/architecture.html`](diagrams/architecture.html), built from [`diagrams/architecture.json`](diagrams/architecture.json) with [archify](https://github.com/tt-a1i/archify). The PNGs in `images/` are screenshots of it in the light and dark themes. To regenerate the HTML, from the repository root:

```bash
node <path-to-archify>/bin/archify.mjs deliver architecture \
  docs/diagrams/architecture.json docs/diagrams/architecture.html --quality showcase
```
