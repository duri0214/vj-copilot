# vj-copilot

Windows の PC 再生音または LINE / MIC 入力を解析し、次に選ぶ映像候補を小さな 2×2 の動画 preview で提示する試作です。標準の WASAPI に加え、任意のビルドで ASIO 入力を利用できます。候補の更新は自動ですが、選択はクリックまたは `1`〜`4` キーで人が行います。PROGRAM 出力や自動切替は行いません。

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

入力機器がなくても確認する場合は、同じ解析経路へ 120 BPM の合成 PCM を流す `--demo` を付けます。音色と音量は 5 秒ごとに低域／高域 × 小／大へ変わり、画面には `DEMO` と表示されます。

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
- 入力の種類とデバイスを選び、「開始」／「停止」でキャプチャを切り替えます。デバイスを変更するときは先に停止します。mono と stereo の f32/f64/i16/i32/u16 PCM に対応し、stereo は mono に平均します。
- メーターは Pioneer DJ ミキサーを意識した緑・黄橙・赤の LED 配色です。画面全体のアクセントは初音ミクをイメージした青緑です。バーは RMS、白線は 1 秒保持するピークです。緑は -12 dBFS 以下、黄橙は -12〜-3 dBFS、赤は -3 dBFS 超、ピークが -0.1 dBFS 以上で `CLIP` を表示します。目盛りはデジタル音声の dBFS で、ミキサー本体の目盛りとは異なります。解析前・停止・切断時は消灯します。PC 再生停止などで新しい解析結果が 700 ms 届かない場合も消灯し、再開時は新しい 1 秒分の音声を待ちます。
- 音量は約 25 ms ごとに集計し、画面は約 16 ms ごとに再描画を要求します。入力デバイスから音声が届く間隔やPCの負荷により、表示更新は遅れる場合があります。
- BPM は数秒分の音から推定します。周期性が弱い間は「推定中」、繰り返し同じテンポを検出すると「安定」と表示します。左のランプは検出した拍で約 100 ms 点灯します。入力停止時に消灯し、PCM が無音のまま続く場合も約 1.5 秒で推定をリセットします。
- 候補枠をクリックするか、`1`〜`4` キーで選択します。選択すると候補の自動更新が止まり、選択した枠と再生位置は維持されます。
- `Space` で選択表示を消し、候補の自動更新を再開します。

### iTunes / Jabra SPEAK 510 で試す

1. iTunes の音楽が Jabra から聞こえることを確認します。
2. VJ Copilot で **PC 再生音** を選び、iTunes と同じ出力先（例: `Speakers (Jabra SPEAK 510 USB)`。名前は環境で異なります）を選びます。
3. 「開始」を押し、音楽を再生します。メーターが動き、1 秒分の有音入力が揃うと候補が表示されます。

`LINE / MIC` に出る `Headset (Jabra SPEAK 510 USB)` は録音側です。同じ機器名でも再生側の音声とは別です。PC 再生音は [Windows WASAPI のループバック](https://learn.microsoft.com/en-us/windows/win32/coreaudio/loopback-recording)で取り込みます。選んだ出力先で鳴るほかのアプリの音も含みます。メーターが動かない場合は、iTunes の出力先と選択したデバイス、開始状態を確認してください。

### BPM の読み方

FFT の正のスペクトル変化（log spectral flux）から立ち上がりの強さを抽出し、直近 4〜8 秒の自己相関で 60〜200 BPM の周期を探します。推定は約 500 ms ごとに更新します。「安定」は信頼度 0.6 以上・3 回連続で差が 2.5% 未満・直近の周期性が続いている場合です。

信頼度は周期性の目安で、正解の確率ではありません。半分・倍のテンポや、キック以外の音を拍として拾うことがあります。DJ ソフトの beat sync 用ではなく、候補検索に利用するための試験的な計測です。現時点の動画ランキングは従来どおり energy / brightness を使用します。

### ASIO 入力を追加したビルド（Windows・任意）

通常ビルドは WASAPI を使い、ASIO SDK や LLVM は不要です。ASIO 対応機器のメーカー製ドライバーを用意した場合に、`asio` feature を追加できます。[CPAL 0.15.3 の ASIO 手順](https://github.com/RustAudio/cpal/tree/v0.15.3#asio-on-windows)に従い、Visual Studio C++ Build Tools と `libclang.dll` を準備します。

`--features asio` はビルド時に必須です。feature を付けない `cargo run --release -- --media-dir .\demo-media` は WASAPI 版として起動するため、画面に ASIO ボタンは表示されません。ASIO を使う場合は、次のように `--features asio` を付けて起動してください。`LIBCLANG_PATH` と `CPAL_ASIO_DIR` は、インストール先または手動配置した SDK の場所に合わせて変更します。

```powershell
# LLVM をこの場所にインストールした場合。実際の libclang.dll のフォルダを指定します。
$env:LIBCLANG_PATH = 'C:\Program Files\LLVM\bin'
# SDK を手動配置する場合のみ、common / host を含むフォルダを指定します。
# $env:CPAL_ASIO_DIR = 'C:\SDKs\asiosdk'
cargo run --release --features asio -- --media-dir .\demo-media
```

exe だけを作る場合は `cargo build --release --features asio` を実行します。出力先は通常版と同じ `target\release\vj-copilot.exe` です。ASIO と WASAPI の両方を使う場合は ASIO 有効版を起動してください。SDK と LLVM はビルド時に必要で、実行時には対応機器の ASIO ドライバーを使います。

`CPAL_ASIO_DIR` が未指定なら依存の `asio-sys` が初回ビルド時に [Steinberg ASIO SDK](https://www.steinberg.net/asiosdk) を取得します。アプリに **WASAPI / ASIO** の切替が現れます。ASIO を選び、ドライバーと入力 ch の先頭番号を指定して「開始」を押してください。指定 ch と次の ch を平均し、最終 ch を指定した場合は mono として取得します。範囲外の ch はエラーを表示します。PC 再生音のループバックは WASAPI 側で選択します。

ドライバーがない場合は「入力デバイスが見つかりません」と表示し、WASAPI へ戻せます。バッファサイズ・サンプルレートはドライバーの既定値を使うため、変更する場合は入力を停止し、機器の設定パネルで変更してから再開します。

### 遅延の検証

「入力タイミング / 検証」を開くと、直近のコールバックの frames 数・音声時間・到着間隔と、PCM 受信から解析完了／描画要求までの経過時間を確認できます。これらは機器の ADC・ドライバー内部・ディスプレイの遅延を含みません。特に `frames ÷ sample_rate` はバッファに含まれる音声の長さで、実測の入力遅延ではありません。表示値はリアルタイムの直近値です。

1. 同じ機器・入力音・サンプルレートで WASAPI と ASIO を比較します。ASIO のバッファを機器の設定で変更し、毎回停止→再開します。
2. 既知のクリック音を LINE 入力し、メーター・BPM・拍ランプ、到着間隔、処理時間、破棄サンプル数を記録します。最低 30 秒程度動かし、音切れや遅延の増大も確認します。
3. 物理的な往復遅延は、インターフェースの出力を LINE 入力へケーブル接続し、DAW 等で送信・録音したクリックのサンプル差から計測します。アプリ画面までの遅延は、基準クリックとランプを同時に記録して別途比較します。往復遅延と片道の入力遅延は区別してください。

| 機器 / ドライバー | backend | sample rate | 設定 buffer | 観測 frames / 間隔 | 受信→解析 / 描画要求 | 破棄数 | 実測往復遅延 |
| --- | --- | --- | --- | --- | --- | --- | --- |
| （実機検証時に記録） | WASAPI / ASIO | Hz | samples | samples / ms | ms / ms | samples | ms |

USB マイクでも PCM・音量・BPM の動作は確認できます。ただし通話用マイクは LINE 入力と特性が異なり、空気経由の音も遅れるため、本番の精度・遅延評価は DJ 卓またはオーディオインターフェースで行います。

## 固定した MVP 定数

設定画面は設けず、次の値を固定しています。

| 項目 | 値 |
| --- | --- |
| 音量メーター | 約 25 ms 窓、RMS / peak |
| 特徴量の解析間隔 | 200 ms |
| BPM | 約 10 ms hop、4〜8 秒窓、500 ms ごとに更新、60〜200 BPM |
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

## 自動検証

```powershell
cargo fmt --all -- --check
cargo test --all-targets
# ASIO のビルド環境を準備した場合
cargo test --all-targets --all-features
cargo clippy --all-targets --all-features -- -D warnings
```

合成 PCM による 90 / 120 / 150 BPM、拍間隔、無音・一定音・ノイズ、テンポ変更、25 ms の音量更新、PCM 変換と入力チャンネル選択、音声欠落時のリセットを検証します。ASIO ドライバーの実入力と物理遅延は自動テストの対象外です。
