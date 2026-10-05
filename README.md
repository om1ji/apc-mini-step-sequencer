**English** | [Русский](README.ru.md)

# APC Seq

A drum machine plugin for the AKAI APC mini mk2. Tap steps on the pad grid, switch drums with the buttons below it, mix with the faders — APC Seq plays your samples in time with the host, no MIDI mapping or Drum Rack needed.

It is a VST3 instrument, tested in Ableton Live 12 on macOS. If you would rather sequence a Drum Rack and see the pattern inside Live's device chain, use [apc-mini-step-sequencer-max-for-live](https://github.com/om1ji/apc-mini-step-sequencer-max-for-live), the Max for Live version.

## What it does

- **16-step patterns for eight drums.** The top two pad rows are one bar of 1/16 steps; each of the eight buttons under the grid shows one drum.
- **Pads light up like a step sequencer should:** gray steps, yellow hits, a green light running along with the host's transport.
- **Plays its own samples.** Drop WAV, AIFF, FLAC or MP3 files onto the plugin window, one per drum. Samples stay with the project.
- **Faders mix the kit.** Faders 1–8 set the volume of each drum, fader 9 the master, with smooth, click-free changes.
- **Tight timing.** Hits land on the exact sample of each step and follow the host's tempo, loops and position changes.
- **No setup on the track.** The plugin talks to the controller itself, so there is no MIDI routing to configure.

## What you need

- AKAI APC mini mk2.
- A VST3 host. Tested only in Ableton Live 12.0.5 on macOS (Apple Silicon).
- macOS on Apple Silicon or Intel. Windows has not been tested.

## Install

1. Download `APC-Seq-0.1.0-macOS-VST3.zip` from the [latest release](https://github.com/om1ji/apc-mini-step-sequencer/releases/latest) and unzip it.
2. Copy `APC Seq.vst3` to `~/Library/Audio/Plug-Ins/VST3` (in Finder: **Go → Go to Folder…**, paste the path; create the `VST3` folder if it is missing).
3. The plugin is not notarized by Apple, so macOS blocks it after download. Unblock it once in Terminal:

   ```bash
   xattr -dr com.apple.quarantine ~/Library/Audio/Plug-Ins/VST3/"APC Seq.vst3"
   ```

### Building from source

With [Rust](https://rustup.rs) installed, from the repository folder:

```bash
cargo xtask bundle apc_seq --release
mkdir -p ~/Library/Audio/Plug-Ins/VST3
ln -sfn "$PWD/target/bundled/APC Seq.vst3" ~/Library/Audio/Plug-Ins/VST3/"APC Seq.vst3"
```

The first build downloads dependencies and takes a few minutes. The link keeps the installed plugin up to date after every rebuild.

## Set up Live

1. **Settings → Plug-Ins:** turn on **Use VST3 Plug-in System Folders** and rescan.
2. **Settings → Link, Tempo & MIDI:** if **APC mini mk2** is selected as a Control Surface, set that slot to **None**, otherwise Live repaints the pads.
3. Create a MIDI track and drag **Plug-Ins → om1ji → APC Seq** onto it. The track's MIDI input does not matter.
4. Open the plugin window and load samples (see below).
5. Press Play and tap the top two pad rows.

## Playing

| Control | What it does |
| --- | --- |
| Top two pad rows | steps 1–8 on the top row, 9–16 on the row below; tap to turn a step on or off |
| Buttons under the grid | pick the drum to edit, 1 to 8 |
| Faders 1–8 | volume of drums 1–8 |
| Fader 9 | master volume |

You can edit steps while the host is playing or stopped. All eight drums play at once, whichever one is shown on the grid. Every hit has the same velocity.

The controller does not report where its faders are until you move them, so until then every fader is treated as being at 80 %.

### Loading samples

Drag a file from Live's browser onto a drum's row in the plugin window — the row lights up while you drag. **Загрузить…** opens a file dialog instead, and **▶** plays the sample.

The plugin window is in Russian:

| Text | Meaning |
| --- | --- |
| `Контроллер подключён` / `Контроллер не найден` | the APC is connected / not found |
| `Перетащите сэмпл из браузера Live на строку дорожки` | drag a sample from Live's browser onto a track row |
| `— пусто —` | no sample loaded |
| `Загрузить…` | load a file |

## If something does not work

| What you see | What to do |
| --- | --- |
| Live does not list APC Seq, or macOS says it cannot be opened | the download is still blocked: run the `xattr` command from [Install](#install), then rescan plug-ins |
| `Контроллер не найден` in the window | check the USB cable; the plugin looks for a MIDI port named like `APC mini mk2 Control` |
| Pads flicker or show clip colors | another script is also driving the lights: the APC mini mk2 control surface in Live, or the Max for Live version of APC Seq |
| Faders do nothing until moved, then the volume jumps | expected: the plugin learns a fader's position from its first move |
| `диалог не открылся — перетащите файл` | the file dialog did not open; drag the file onto the row instead |
| `неизвестный формат`, `кодек не поддерживается` or `в файле нет аудио` | the file could not be read; convert it to WAV |
| A drum is silent after moving sample files | samples are remembered by their location on disk; load the file again |

## Limitations

- The pattern is not saved with the project yet. Samples are; the steps start empty after reopening.
- One bar of 16 steps in 4/4. Longer patterns and other meters are not supported yet.
- Windows has not been tested. Windows usually lets only one program use a MIDI port, so the plugin may not see the controller while Live has it open.
- The right-hand buttons and the lower six pad rows are not used.
- Use either this plugin or the Max for Live version on a controller, not both at once.

## How it works

The internals — threads, design decisions, the controller protocol and tests — are in [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).

## License

[MIT](LICENSE).

> [!NOTE]
> The VST3 support code in NIH-plug is GPLv3, so if you share a compiled `APC Seq.vst3`, you must do so under the GPLv3. The source code in this repository is MIT.

[`vendor/egui-baseview`](vendor/egui-baseview) is a modified copy of [egui-baseview](https://github.com/BillyDM/egui-baseview), MIT, with its original [LICENSE](vendor/egui-baseview/LICENSE).
