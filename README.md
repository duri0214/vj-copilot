# vj-copilot

Windows の PC 再生音または LINE / MIC 入力を解析し、次に選ぶ映像候補を小さな 2×2 の動画 preview で提示する試作です。候補の更新は自動ですが、選択はクリックまたは `1`〜`4` キーで人が行います。PROGRAM 出力や自動切替は行いません。

## 必要なもの

- Windows と Rust の stable MSVC toolchain
- `ffmpeg` コマンドが PATH から実行できること

Rust は [rustup](https://rustup.rs/) で導入できます。導入後、PowerShell で次を確認してください。

```powershell
cargo --version
rustc --version
ffmpeg -version
```

`ffmpeg` は素材の先頭最大 3 秒を 320×180・15 fps・RGBA のメモリ内フレームへ変換するために使います。入力音と MP4 の音声は再生しません。

## ダミー素材の生成と起動

リポジトリ直下で、まず 8 本のダミー MP4 を生成します。生成物は Git 管理外です。

```powershell
.\scripts\generate-demo-media.ps1
```

通常起動では、「PC 再生音」または「LINE / MIC」を選び、デバイスを指定して「開始」を押します。

```powershell
cargo run --release -- --media-dir .\demo-media
```

入力機器がなくても確認する場合は、同じ解析経路へ 5 秒ごとに低域／高域 × 小／大音量の合成 PCM を流す `--demo` を付けます。画面には `DEMO` と表示されます。

```powershell
cargo run --release -- --media-dir .\demo-media --demo
```

## リリース exe の作成と直接起動

`cargo run --release` は、ソースや依存関係に変更がある場合だけリビルドし、完了後にアプリを起動します。exe だけを作成する場合は、次を実行します。

```powershell
cargo build --release
```

リポジトリ直下の `target\release\vj-copilot.exe` が作成されます。以後は Cargo を経由せず、exe を直接起動できます。

```powershell
.\target\release\vj-copilot.exe --media-dir .\demo-media
```

デモ入力を使う場合は、起動引数に `--demo` を追加します。相対パスを使うため、コマンドはリポジトリ直下で実行してください。ショートカットやバッチファイルから起動する場合も、作業フォルダをリポジトリ直下に設定します。

## 操作

- ダークな操作パネルの上部が音声入力と音量メーター、下部が映像候補です。各候補は左に小さな動画、右に番号・素材名・選択状態を表示します。未割り当ては `Nothing` です。
- 入力の種類とデバイスを選び、「開始」／「停止」でキャプチャを切り替えます。デバイスを変更するときは先に停止します。mono と stereo の f32/i16/u16 PCM に対応し、stereo は mono に平均します。
- メーターのバーは RMS、白線は 1 秒保持するピークです。緑は -12 dBFS 以下、黄は -12〜-3 dBFS、赤は -3 dBFS 超、ピークが -0.1 dBFS 以上で `CLIP` を表示します。解析前・停止・切断時は消灯します。PC 再生停止などで新しい解析結果が 700 ms 届かない場合も消灯し、再開時は新しい 1 秒分の音声を待ちます。
- 候補枠をクリックするか、`1`〜`4` キーで選択します。選択すると候補更新は保留され、枠と再生位置は維持されます。
- `Space` または `AUTO / 保留`・`HOLD / 解除` ボタンで保留と解除を切り替えます。解除時は選択表示を消し、候補更新を再開します。

### iTunes / Jabra SPEAK 510 で試す

1. iTunes の音楽が Jabra から聞こえることを確認します。
2. VJ Copilot で **PC 再生音** を選び、iTunes と同じ出力先（例: `Speakers (Jabra SPEAK 510 USB)`。名前は環境で異なります）を選びます。
3. 「開始」を押し、音楽を再生します。メーターが動き、1 秒分の有音入力が揃うと候補が表示されます。

`LINE / MIC` に出る `Headset (Jabra SPEAK 510 USB)` は録音側です。同じ機器名でも再生側の音声とは別です。PC 再生音は [Windows WASAPI のループバック](https://learn.microsoft.com/en-us/windows/win32/coreaudio/loopback-recording)で取り込みます。選んだ出力先で鳴るほかのアプリの音も含みます。メーターが動かない場合は、iTunes の出力先と選択したデバイス、開始状態を確認してください。

## 固定した MVP 定数

設定画面は設けず、次の値を固定しています。

| 項目 | 値 |
| --- | --- |
| 解析間隔 | 200 ms |
| centroid | 直近 2048 samples、Hann 窓、振幅スペクトル |
| energy | `clamp((dBFS + 60) / 60, 0, 1)` |
| brightness | `clamp(centroid_hz / 8000, 0, 1)` |
| 検索に使う特徴 | 有音の直近 1 秒（5 区間）の平均 |
| 候補更新 | 2 秒ごと |
| 素材・候補 | 最大 8 本・上位 4 本 |
| preview | デコードは 320×180、15 fps、先頭最大 3 秒（最大 45 フレーム）。画面表示は 112×63 logical px（幅約 3 cm が目安。実寸は画面の DPI・拡大率による） |

`clips.json` は指定した素材フォルダ直下に置きます。`file` は同じフォルダの MP4 ファイル名、`energy` と `brightness` は有限の 0〜1 の数値です。

```json
[
  { "file": "low_dark_01.mp4", "energy": 0.2, "brightness": 0.2 },
  { "file": "high_bright_01.mp4", "energy": 0.8, "brightness": 0.8 }
]
```

無音、入力停止・切断中は候補を保持します。`clips.json`、MP4、FFmpeg に問題がある場合は、原因を画面に表示したまま操作できる状態を保ちます。
