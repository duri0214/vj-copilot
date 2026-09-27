# vj-copilot

Windows の PC 再生音または LINE / MIC 入力を解析し、次に選ぶ Background 映像を 4 本まで提示する VJ 支援アプリです。候補は自動更新され、映像の選択は利用者が行います。選択した Background と透過 Foreground は、独立した STAGE ウィンドウに表示されます。

## 1. 事前準備

次の環境を用意します。

- Windows
- Rust stable（MSVC toolchain）
- FFmpeg（`ffmpeg` コマンドを PATH から実行できること）

Rust は [rustup](https://rustup.rs/) から導入できます。PowerShell で各ソフトが利用できることを確認してください。

```powershell
rustc --version
cargo --version
ffmpeg -version
```

リポジトリを取得したら、PowerShell でリポジトリ直下へ移動します。以降のコマンドはすべてリポジトリ直下で実行します。

## 2. Background 素材を生成する

次のスクリプトは、指定した出力先（省略時は `demo-media`）の `background` に 8 本のサンプル MP4 と `background-metadata.json` を生成します。`demo-media` はサンプル素材を置くフォルダ名の一例です。

```powershell
.\scripts\generate-background-media.ps1
```

同梱の `demo-media\foreground\neon-prism-right.png` と合わせて、Background と Foreground の表示をすぐに試せます。生成した MP4 は Git 管理外です。別のフォルダへ生成する場合は `-OutputDir` を指定します。

## 3. 起動する

実際の PC 再生音または LINE / MIC 入力を使う場合は、次のコマンドで起動します。

```powershell
cargo run --release -- --media-dir .\demo-media
```

入力機器を使わずに画面と解析を確認する場合は `--demo` を付けます。合成音声の特徴が約 5 秒ごとに変わり、候補の更新を確認できます。

```powershell
cargo run --release -- --media-dir .\demo-media --demo
```

リリース exe だけを作る場合は、次を実行します。

```powershell
cargo build --release
```

作成された exe は、リポジトリ直下から直接起動できます。

```powershell
.\target\release\vj-copilot.exe --media-dir .\demo-media
.\target\release\vj-copilot.exe --media-dir .\demo-media --demo
```

別の素材フォルダを使う場合は、`.\demo-media` をそのフォルダのパスへ置き換えてください。

## 4. 素材を配置する

`--media-dir` で指定するフォルダは、次の構成にします。フォルダがない場合は起動時に作成されます。

```text
media-dir/
├─ background/
│  ├─ background-metadata.json
│  ├─ background-01.mp4
│  └─ background-02.mp4
└─ foreground/
   └─ foreground-01.png
```

### Background

Background は `background` 直下へ MP4 と `background-metadata.json` を置きます。子フォルダ内の MP4 は読み込みません。MP4 は FFmpeg で読み込める形式にしてください。

`background-metadata.json` は、各 MP4 の特徴を `energy`（勢い）と `brightness`（明るさ）で表すファイルです。どちらも `0` から `1` までの数値で指定します。`file` には同じ `background` フォルダにある MP4 のファイル名だけを指定してください。

```json
[
  { "file": "background-01.mp4", "energy": 0.2, "brightness": 0.2 },
  { "file": "background-02.mp4", "energy": 0.8, "brightness": 0.8 }
]
```

メタデータがない MP4、存在しない MP4 を参照する項目、範囲外の値を持つ項目は候補から除外されます。

### Foreground

Foreground は `foreground` 直下へ、1920×1080 の RGBA PNG を置きます。透明キャンバス上での配置が、そのまま STAGE 上の配置になります。子フォルダ内の PNG、サイズが異なる PNG、読み込めない PNG は候補から除外されます。

STAGE には Foreground を 1 枚だけ表示できます。別の画像を選ぶと置き換わり、`CLEAR` で Background だけの表示へ戻ります。

## 5. 操作する

通常起動では、入力の種類を `PC 再生音` または `LINE / MIC` から選び、デバイスを指定して `開始` を押します。デバイスを変更するときは、先に入力を停止してください。

- Background 候補をクリックするか、`1`〜`4` キーで選択します。選択中は候補の自動更新が止まります。
- `Space` で Background の選択を解除し、候補の自動更新を再開します。
- Foreground 候補をクリックして選択し、`CLEAR` で解除します。
- STAGE ウィンドウを閉じた場合は、操作画面の `OPEN STAGE WINDOW` から再表示できます。
- STAGE ウィンドウはプロジェクタやサブディスプレイへ移動できます。

PC で再生中の音を使う場合は、音楽を出している Windows の出力先と、アプリの `PC 再生音` で選ぶデバイスを合わせます。`LINE / MIC` は録音入力を使う場合に選びます。

## 6. 問題があるとき

アプリは素材や入力を読み込めない場合、操作画面に原因を表示します。次を確認してください。

- 素材が表示されない: `--media-dir`、フォルダ構成、ファイル名、`background-metadata.json` の JSON と値を確認します。
- MP4 が除外される: FFmpeg で MP4 を開けることと、メタデータの `file` が実際のファイル名と一致することを確認します。
- Foreground が除外される: PNG が `foreground` 直下にあり、1920×1080 の RGBA 画像であることを確認します。
- 音量メーターが動かない: 入力を開始したこと、選択したデバイスで音が流れていること、Windows 側でデバイスが利用可能なことを確認します。
- `ffmpeg` を起動できない: 新しい PowerShell で `ffmpeg -version` が成功するよう PATH を設定します。
- 候補が `Nothing` のまま: 有効な MP4 が 1 本以上読み込まれていることと、音声入力が届いていることを確認します。4 本未満の場合、不足する枠は `Nothing` になります。

入力停止中、切断中、無音時は現在の候補を保持します。素材の一部が不正でも、読み込めた素材と操作画面はそのまま利用できます。

## ASIO 入力を使う場合（任意）

通常ビルドは Windows の WASAPI を使用するため、ASIO SDK は不要です。ASIO 対応版を作る場合は、Visual Studio C++ Build Tools、ASIO SDK、LLVM の `libclang.dll` を次の場所へ配置します。

```text
tools/
├─ asio-sdk/             # common/ と host/ を含む ASIO SDK のルート
└─ llvm/
   └─ bin/
      └─ libclang.dll
```

配置後、次のスクリプトで WASAPI と ASIO の両方を含む exe を作成します。

```powershell
.\scripts\build-asio.ps1
```

ASIO 対応機器のメーカー製ドライバーは実行時にも必要です。SDK、LLVM、`libclang.dll` は実行時には不要です。

## 開発時の確認

```powershell
cargo fmt --all -- --check
cargo test --all-targets
cargo clippy --all-targets --all-features -- -D warnings
```
