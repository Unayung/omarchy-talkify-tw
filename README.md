<h1 align="center">Talkify for Omarchy（台灣版）</h1>

<h3 align="center">給台灣工程師用的語音輸入：中文夾英文，直接講</h3>

<p align="center">
  <img src=".github/assets/hero.jpg" width="100%" alt="Omarchy 桌面中央的 Talkify 膠囊" />
</p>

按住右 Alt，把想打的字講出來，放開。文字會出現在你原本在打字的視窗：終端機、瀏覽器、編輯器、聊天室都可以。

這是 [InsaneArts/talkify-omarchy](https://github.com/InsaneArts/talkify-omarchy) 的 fork。原版的介面和操作都保留，差別在語音模型：原版用的 Parakeet 只懂 25 種歐洲語言，這裡換成聯發科的 [Breeze ASR 25](https://huggingface.co/MediaTek-Research/Breeze-ASR-25)，專門為台灣華語和中英夾雜訓練的 Whisper。辨識全程在你的電腦上跑，不連雲端、不用帳號。

## 跟原版的差別

- **聽得懂中英夾雜**：「請幫我 review 這個 PR，然後把 deploy 的 script 改掉」會照你講的寫出來。
- **中英之間自動留空白**：中文字之間不加空白，中文和英文、數字之間加一格。
- **詞庫會提示給模型**：Vocabulary 裡的詞（Kubernetes、Hyprland、你的專案名）會先告訴模型，從源頭就拼對。預設已放一批工程師常用詞。
- **中文口語指令**（前後各停頓一下再說）：

  | 說 | 效果 |
  |---|---|
  | 逗號、句號、問號、驚嘆號、冒號、頓號 | 換成對應的全形標點 |
  | 換行、新段落 | 換行 |
  | 刪掉、劃掉 | 刪除前一句 |
  | 送出 | 在終端機裡按下 Enter |

- **詞庫修正支援中文**：`歐瑪奇 -> Omarchy` 這種修正在整句中文裡也比對得到。

## 需求

- Omarchy 4，Hyprland 0.56 以上。
- 建議有 GPU。NVIDIA 裝 `cuda` 套件後會用 CUDA 編譯，RTX 3080 上辨識一句話約 0.4 秒，可以邊講邊顯示草稿。其他顯卡裝 `vulkan-headers` 和 `shaderc` 會用 Vulkan。兩者都沒有就用 CPU，能用但慢，這時即時草稿會自動停用。

## 安裝

```sh
git clone https://github.com/Unayung/omarchy-talkify-tw.git
cd omarchy-talkify-tw
./install.sh
```

`install.sh` 會從原始碼編譯到 `~/.local`，第一次要編 whisper.cpp，CUDA 版大約十分鐘。第一次啟動會下載語音模型（1 GB），下載期間 bar 上的小鬼會慢慢填滿。想指定後端可以用 `TALKIFY_GPU=cuda|vulkan|cpu ./install.sh`。

## 用哪個鍵

**建議用 Omarchy 內建的聽寫鍵**，鍵盤沒有右 Super 也能用。把 Talkify 設成 Omarchy 的聽寫後端：

```sh
omarchy-dictation-use talkify && hyprctl reload
```

之後按住右 Alt（或 F9）說話、放開出字，Super + Ctrl + X 切換免持模式。要換回原本的後端就執行 `omarchy-dictation-use voxtype`。

原版的做法是 Talkify 自己讀鍵盤，預設鍵是右 Super，可以在設定的 Trigger 頁換成別的鍵。這需要帳號在 `input` 群組：

```sh
sudo usermod -aG input "$USER"   # 加完要登出再登入
```

**有裝 keyd 的話，右邊的修飾鍵預設收不到。** keyd 會把右 Ctrl、右 Alt、右 Super 都當成左邊那顆送出，Talkify 看到的永遠是 `KEY_LEFTCTRL`。在 `/etc/keyd/default.conf` 的 `[main]` 加一行讓它照原樣送出，再執行 `sudo keyd reload`：

```ini
rightcontrol = rightcontrol
```

想確認某顆鍵實際送出什麼，可以用 `sudo keyd monitor` 或 `evtest` 看。

以下是原版的功能說明，在這個 fork 都照常運作。

## Appearance

The HUD sits where you want it: a pill at the bottom, a caption on the window you are dictating into, a halo around it, an island in the bar, a mark by the pointer, or a notch from the top edge. It takes its colours from your Omarchy theme and changes the moment you switch.

<p align="center">
  <img src=".github/assets/feature-appearance.jpg" width="100%" alt="Six views of an Omarchy desktop, each in another theme, one per HUD placement: pill, window, halo, island, pointer and notch" />
</p>

## Vocabulary

Teach Talkify the names and words you use. When it gets one wrong, fix it, select your fix and run `talkify teach`, and from then on it writes it your way. Words match by how they sound as well as how they are spelled, and a short phrase can expand into a whole snippet.

<p align="center">
  <img src=".github/assets/feature-vocabulary.jpg" width="100%" alt="The Vocabulary page in Gruvbox beside the pill, where the misheard 'omachi' is struck through and replaced with 'Omarchy'" />
</p>

## Insights

How much you dictate, how fast, and how much typing it saved, with a sixteen-week heatmap in your theme's colours. The ghost celebrates each milestone. Only daily totals are kept, never the text.

<p align="center">
  <img src=".github/assets/feature-insights.jpg" width="100%" alt="The Insights page in Ristretto, with a coral heatmap, beside the ghost celebrating 10,000 words among sparkles" />
</p>

## App rules

Each app can have its own rules: whether to tidy the text, end with a period, write in lower case, or press Enter. Terminals come with one already, so saying "send it" at the end submits your prompt to a coding agent.

<p align="center">
  <img src=".github/assets/feature-app-rules.jpg" width="100%" alt="The App rules page in a light Rosé Pine theme beside the pill, where 'Run the tests and fix what fails' ends in a pressed Enter key" />
</p>

## Using it

- Hold Right Super to talk, let go to insert. Tap it for hands-free, tap again to finish. Escape cancels.
- Click the ghost in the bar for settings, right-click it to start or finish a session.
- `talkify paste-last` pastes your last dictation again, `talkify retry` decodes it again, and `talkify status` shows what Talkify is doing.

To update, run `git pull && ./install.sh`.

To remove Talkify:

```sh
systemctl --user disable --now talkify.service
omarchy plugin remove tornikegomareli.talkify
rm -f ~/.local/bin/talkify ~/.config/systemd/user/talkify.service   # after install.sh
rm -rf ~/.config/talkify ~/.local/share/talkify                      # settings, history, the model
```

## Development

```sh
cargo test                       # the pure rules in crates/talkify-core
cargo build --release -p talkify --features cuda
python3 tests/e2e.py             # a second daemon, real speech, every feature
quickshell -p plugin/dev         # the HUD, playing scripted sessions
```

The domain language is in [CONTEXT.md](CONTEXT.md) and the daemon's protocol in [docs/protocol.md](docs/protocol.md). The pictures here and the product film are code too, in [video](video).

## License

[MIT License](LICENSE). The bundled sounds are listed in [sounds/LICENSE-SOUNDS.txt](sounds/LICENSE-SOUNDS.txt).
