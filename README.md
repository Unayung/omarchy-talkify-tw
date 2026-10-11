<h1 align="center">Talkify for Omarchy（台灣版）</h1>

<h3 align="center">給台灣工程師用的語音輸入：中文夾英文，直接講</h3>

<p align="center"><b>繁體中文</b> · <a href="README.en.md">English</a></p>

<p align="center">
  <img src=".github/assets/hero.jpg" width="100%" alt="Omarchy 桌面中央的 Talkify 膠囊" />
</p>

按住聽寫鍵，把想打的字講出來，放開。文字會出現在你原本在打字的視窗：終端機、瀏覽器、編輯器、聊天室都可以。

這是 [InsaneArts/talkify-omarchy](https://github.com/InsaneArts/talkify-omarchy) 的 fork，原作者是 Tornike Gomareli。原版的介面和操作都保留，差別在語音模型：原版用的 Parakeet 只懂 25 種歐洲語言，這裡換成聯發科研究院的 [Breeze ASR 25](https://huggingface.co/MediaTek-Research/Breeze-ASR-25)，專門為台灣華語和中英夾雜訓練的 Whisper。辨識全程在你的電腦上跑，不連雲端、不用帳號。

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
- **可以用 Omarchy 內建的聽寫鍵**，不必另外設定觸發鍵。

## 需求

- Omarchy 4，Hyprland 0.56 以上。
- 建議有 GPU。NVIDIA 裝 `cuda` 套件後會用 CUDA 編譯，RTX 3080 上辨識一句話約 0.4 秒，可以邊講邊顯示草稿。其他顯卡裝 `vulkan-headers` 和 `shaderc` 會用 Vulkan。兩者都沒有就用 CPU，能用但慢，這時即時草稿會自動停用。目前只有 CUDA 實際測過。

## 安裝

```sh
git clone https://github.com/Unayung/omarchy-talkify-tw.git
cd omarchy-talkify-tw
./install.sh
```

`install.sh` 會從原始碼編譯到 `~/.local`，第一次要編 whisper.cpp，CUDA 版大約十分鐘。第一次啟動會下載語音模型（1 GB），下載期間 bar 上的小鬼會慢慢填滿。想指定後端可以用 `TALKIFY_GPU=cuda|vulkan|cpu ./install.sh`。

## 用哪個鍵

**方法一：Omarchy 內建的聽寫鍵。** 把 Talkify 設成 Omarchy 的聽寫後端：

```sh
omarchy-dictation-use talkify && hyprctl reload
```

之後按住右 Alt（或 F9）說話、放開出字，Super + Ctrl + X 切換免持模式。要換回原本的後端就執行 `omarchy-dictation-use voxtype`。

**方法二：Talkify 自己的觸發鍵。** 預設是右 Super，可以在設定的 Trigger 頁換成別的鍵。這需要 Talkify 直接讀鍵盤，所以帳號要在 `input` 群組：

```sh
sudo usermod -aG input "$USER"   # 加完要登出再登入
```

**有裝 keyd 的話，右邊的修飾鍵預設收不到。** keyd 會把右 Ctrl、右 Alt、右 Super 都當成左邊那顆送出，Talkify 看到的永遠是 `KEY_LEFTCTRL`。在 `/etc/keyd/default.conf` 的 `[main]` 加一行讓它照原樣送出，再執行 `sudo keyd reload`：

```ini
rightcontrol = rightcontrol
```

想確認某顆鍵實際送出什麼，可以用 `sudo keyd monitor` 或 `evtest` 看。

## 外觀

HUD 可以放在六個位置：底部的膠囊、貼在正在輸入的視窗上的字幕、視窗四周的光暈、bar 裡的小島、游標旁的標記，或從頂端垂下的瀏海。顏色跟著你的 Omarchy 主題，換主題時立刻跟著換。

<p align="center">
  <img src=".github/assets/feature-appearance.jpg" width="100%" alt="六個 Omarchy 桌面畫面，各用不同主題，展示六種 HUD 位置" />
</p>

## 詞庫

把你常用的名字和詞教給 Talkify。聽錯的時候，改好、選取改過的字、執行 `talkify teach`，之後它就會照你的寫法。英文的詞會同時比對發音和拼字，短句也可以展開成一整段片語。詞庫是純文字檔，在 `~/.config/talkify/words.txt`：

```text
Hyprland                 偏好的寫法（大小寫照這裡，也會提示給語音模型）
歐瑪奇 -> Omarchy         修正：聽到左邊，寫成右邊
我的信箱 => me@x.dev      片語：說左邊，貼上右邊
```

<p align="center">
  <img src=".github/assets/feature-vocabulary.jpg" width="100%" alt="Vocabulary 頁面，聽錯的 omachi 被劃掉並換成 Omarchy" />
</p>

## Insights

你講了多少、多快、省了多少打字時間，加上一張十六週的熱力圖。每到一個里程碑小鬼會慶祝一下。只保留每天的總數，不保留文字內容。中文以字計數，英文以詞計數。

<p align="center">
  <img src=".github/assets/feature-insights.jpg" width="100%" alt="Insights 頁面的熱力圖，旁邊是慶祝一萬字的小鬼" />
</p>

## App rules

每個 app 可以有自己的規則：要不要整理文字、要不要句尾句點、要不要全小寫、要不要按 Enter。終端機預設已有一條規則，所以最後說「送出」就會把提示送給 coding agent。

<p align="center">
  <img src=".github/assets/feature-app-rules.jpg" width="100%" alt="App rules 頁面，句尾接著一個被按下的 Enter 鍵" />
</p>

## 操作

- 按住聽寫鍵說話，放開就插入。點一下進入免持模式，再點一下結束。Escape 取消。
- 點 bar 上的小鬼開設定，右鍵點它可以開始或結束一段聽寫。
- `talkify paste-last` 把上一段再貼一次，`talkify retry` 重新辨識一次，`talkify status` 顯示目前狀態。
- 沒反應的時候先看麥克風有沒有被靜音。

更新：`git pull && ./install.sh`。

移除：

```sh
systemctl --user disable --now talkify.service
omarchy plugin remove tornikegomareli.talkify
rm -f ~/.local/bin/talkify ~/.local/bin/omarchy-dictation-talkify ~/.config/systemd/user/talkify.service
rm -rf ~/.config/talkify ~/.local/share/talkify    # 設定、歷史紀錄、模型
```

## 開發

```sh
cargo test                                          # crates/talkify-core 的純規則
cargo build --release -p talkify --features cuda
python3 tests/e2e.py                                # 第二個 daemon、真的語音、每個功能
quickshell -p plugin/dev                            # HUD，播放預錄的情境
```

領域用語在 [CONTEXT.md](CONTEXT.md)，daemon 的協定在 [docs/protocol.md](docs/protocol.md)。e2e 的中文語音用 `edge-tts` 合成，沒裝的話那幾個測試會跳過。

## 授權與致謝

程式碼採 [MIT 授權](LICENSE)，著作權屬於原作者 Tornike Gomareli 和這個 fork 的作者 Chen Chia Yang。

- **原版**：[InsaneArts/talkify-omarchy](https://github.com/InsaneArts/talkify-omarchy)，MIT。本頁的圖片來自原版，畫面上是英文版的功能。
- **語音模型**：[Breeze ASR 25](https://huggingface.co/MediaTek-Research/Breeze-ASR-25)，聯發科研究院釋出，Apache 2.0。這個 repo 不包含模型檔，安裝時會從 [alan314159/Breeze-ASR-25-whispercpp](https://huggingface.co/alan314159/Breeze-ASR-25-whispercpp)（GGML 轉檔，Apache 2.0）下載。
- **推論引擎**：[whisper.cpp](https://github.com/ggml-org/whisper.cpp)（MIT），透過 [whisper-rs](https://codeberg.org/tazz4843/whisper-rs)。
- **音效**：列在 [sounds/LICENSE-SOUNDS.txt](sounds/LICENSE-SOUNDS.txt)。

「Talkify」是原作者的專案名稱，這個 fork 與 [usetalkify.app](https://usetalkify.app) 沒有隸屬關係。
