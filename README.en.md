<h1 align="center">Talkify for Omarchy (Taiwan edition)</h1>

<h3 align="center">Voice dictation for Taiwanese engineers: Mandarin with English mixed in</h3>

<p align="center"><a href="README.md">繁體中文</a> · <b>English</b></p>

<p align="center">
  <img src=".github/assets/hero.jpg" width="100%" alt="The Talkify pill in the middle of an Omarchy desktop" />
</p>

Hold the dictation key, say what you want to write, and let go. The words land in the window you were typing in: a terminal, a browser, an editor, a chat.

This is a fork of [InsaneArts/talkify-omarchy](https://github.com/InsaneArts/talkify-omarchy) by Tornike Gomareli. The interface and the way it works are upstream's. What changed is the Speech Model: upstream's Parakeet understands 25 European languages and no Chinese, so this fork runs MediaTek Research's [Breeze ASR 25](https://huggingface.co/MediaTek-Research/Breeze-ASR-25), a Whisper tuned for Taiwanese Mandarin and for English spoken inside it. Speech is recognised on your machine. No cloud, no account.

## What differs from upstream

- **Mandarin with English mixed in.** "請幫我 review 這個 PR，然後把 deploy 的 script 改掉" is written the way you said it.
- **Spacing.** No spaces between Chinese characters, one space between Chinese and English or numerals.
- **The Vocabulary is a hint to the model.** Words you add (Kubernetes, Hyprland, your project's name) are given to the Speech Model before it listens, so it spells them right as it hears them. The seed vocabulary is the English an engineer says every day.
- **Spoken commands in Chinese**, said on their own between two pauses:

  | Say | It does |
  |---|---|
  | 逗號, 句號, 問號, 驚嘆號, 冒號, 頓號 | the full-width mark |
  | 換行, 新段落 | a line break |
  | 刪掉, 劃掉 | removes the phrase before it |
  | 送出 | presses Enter, in a terminal |

  The English ones (comma, new line, scratch that, send it) still work.
- **Corrections match inside Chinese.** `歐瑪奇 -> Omarchy` is found in the middle of a sentence, where there are no spaces to make words of.
- **Omarchy's own dictation keys** can drive it, with no trigger key to set up.

## Requirements

- Omarchy 4 with Hyprland 0.56 or newer.
- A GPU, ideally. With the `cuda` package installed on NVIDIA the build uses CUDA: about 0.4 seconds a sentence on an RTX 3080, fast enough to show a live draft as you speak. Other cards build with Vulkan when `vulkan-headers` and `shaderc` are installed. With neither it runs on the CPU, which works but is slow, and the live draft turns itself off. Only the CUDA build has been tested so far.

## Install

```sh
git clone https://github.com/Unayung/omarchy-talkify-tw.git
cd omarchy-talkify-tw
./install.sh
```

`install.sh` builds from source into `~/.local`. The first build compiles whisper.cpp, about ten minutes with CUDA. The first start downloads the Speech Model (1 GB) while the ghost in the bar fills up. `TALKIFY_GPU=cuda|vulkan|cpu ./install.sh` chooses the backend by hand.

## Which key

**Omarchy's dictation keys.** Make Talkify Omarchy's dictation backend:

```sh
omarchy-dictation-use talkify && hyprctl reload
```

Then hold Right Alt (or F9) to talk and let go to insert, or press Super + Ctrl + X for hands-free. `omarchy-dictation-use voxtype` goes back.

**Talkify's own trigger.** Right Super by default; pick another on the Trigger page in settings. Talkify reads the keyboard itself for this, which needs the `input` group:

```sh
sudo usermod -aG input "$USER"   # then log out and back in
```

**With keyd, the right-hand modifiers never arrive.** keyd sends Right Ctrl, Right Alt and Right Super as their left-hand keys, so all Talkify ever sees is `KEY_LEFTCTRL`. Add a line under `[main]` in `/etc/keyd/default.conf` to send the key as itself, then run `sudo keyd reload`:

```ini
rightcontrol = rightcontrol
```

`sudo keyd monitor` or `evtest` shows what a key really sends.

## Appearance

The HUD sits where you want it: a pill at the bottom, a caption on the window you are dictating into, a halo around it, an island in the bar, a mark by the pointer, or a notch from the top edge. It takes its colours from your Omarchy theme and changes the moment you switch.

<p align="center">
  <img src=".github/assets/feature-appearance.jpg" width="100%" alt="Six views of an Omarchy desktop, each in another theme, one per HUD placement: pill, window, halo, island, pointer and notch" />
</p>

## Vocabulary

Teach Talkify the names and words you use. When it gets one wrong, fix it, select your fix and run `talkify teach`, and from then on it writes it your way. English words match by how they sound as well as how they are spelled, and a short phrase can expand into a whole snippet. It is a plain text file, `~/.config/talkify/words.txt`:

```text
Hyprland                 a word to prefer: spelled like this, and a hint to the model
歐瑪奇 -> Omarchy         a correction: heard, then written
我的信箱 => me@x.dev      a snippet: say the left, get the right
```

<p align="center">
  <img src=".github/assets/feature-vocabulary.jpg" width="100%" alt="The Vocabulary page beside the pill, where the misheard 'omachi' is struck through and replaced with 'Omarchy'" />
</p>

## Insights

How much you dictate, how fast, and how much typing it saved, with a sixteen-week heatmap in your theme's colours. The ghost celebrates each milestone. Only daily totals are kept, never the text. Chinese counts by the character, English by the word.

<p align="center">
  <img src=".github/assets/feature-insights.jpg" width="100%" alt="The Insights page with its heatmap, beside the ghost celebrating 10,000 words" />
</p>

## App rules

Each app can have its own rules: whether to tidy the text, end with a period, write in lower case, or press Enter. Terminals come with one already, so saying "送出" or "send it" at the end submits your prompt to a coding agent.

<p align="center">
  <img src=".github/assets/feature-app-rules.jpg" width="100%" alt="The App rules page beside the pill, where a sentence ends in a pressed Enter key" />
</p>

## Using it

- Hold the dictation key to talk, let go to insert. Tap it for hands-free, tap again to finish. Escape cancels.
- Click the ghost in the bar for settings, right-click it to start or finish a session.
- `talkify paste-last` pastes your last dictation again, `talkify retry` decodes it again, and `talkify status` shows what Talkify is doing.
- When nothing happens, check that the microphone is not muted.

To update, run `git pull && ./install.sh`.

To remove Talkify:

```sh
systemctl --user disable --now talkify.service
omarchy plugin remove tornikegomareli.talkify
rm -f ~/.local/bin/talkify ~/.local/bin/omarchy-dictation-talkify ~/.config/systemd/user/talkify.service
rm -rf ~/.config/talkify ~/.local/share/talkify    # settings, history, the model
```

## Development

```sh
cargo test                                          # the pure rules in crates/talkify-core
cargo build --release -p talkify --features cuda
python3 tests/e2e.py                                # a second daemon, real speech, every feature
quickshell -p plugin/dev                            # the HUD, playing scripted sessions
```

The domain language is in [CONTEXT.md](CONTEXT.md) and the daemon's protocol in [docs/protocol.md](docs/protocol.md). The Mandarin in the end-to-end tests is synthesized with `edge-tts`; without it those tests are skipped.

## License and credits

The code is under the [MIT License](LICENSE), copyright Tornike Gomareli, who wrote Talkify, and Chen Chia Yang, who wrote this fork.

- **Upstream**: [InsaneArts/talkify-omarchy](https://github.com/InsaneArts/talkify-omarchy), MIT. The pictures on this page are upstream's and show the English it was built for.
- **Speech Model**: [Breeze ASR 25](https://huggingface.co/MediaTek-Research/Breeze-ASR-25) by MediaTek Research, Apache 2.0. This repository does not contain the model; it is downloaded on first start from [alan314159/Breeze-ASR-25-whispercpp](https://huggingface.co/alan314159/Breeze-ASR-25-whispercpp), a GGML conversion under Apache 2.0.
- **Inference**: [whisper.cpp](https://github.com/ggml-org/whisper.cpp) (MIT), through [whisper-rs](https://codeberg.org/tazz4843/whisper-rs).
- **Sounds**: listed in [sounds/LICENSE-SOUNDS.txt](sounds/LICENSE-SOUNDS.txt).

"Talkify" is the name of the original author's project. This fork is not affiliated with [usetalkify.app](https://usetalkify.app).
