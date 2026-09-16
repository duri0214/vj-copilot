# vj-copilot

Windows の LINE 入力を解析し、次に選ぶ映像候補を 2×2 の動画 preview で提示する試作です。候補の更新は自動ですが、選択はクリックまたは `1`〜`4` キーで人が行います。PROGRAM 出力や自動切替は行いません。

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

通常起動では、画面から LINE 入力デバイスを選び「開始」を押します。

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

- 入力デバイスを選び、「開始」／「停止」でキャプチャを切り替えます。mono と stereo の f32/i16/u16 PCM に対応し、stereo は mono に平均します。
- 候補枠をクリックするか、`1`〜`4` キーで選択します。選択すると候補更新は保留され、枠と再生位置は維持されます。
- `Space` または「候補更新を保留」ボタンで保留と解除を切り替えます。解除時は選択表示を消し、候補更新を再開します。

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
| preview | 320×180、15 fps、先頭最大 3 秒（最大 45 フレーム） |

`clips.json` は指定した素材フォルダ直下に置きます。`file` は同じフォルダの MP4 ファイル名、`energy` と `brightness` は有限の 0〜1 の数値です。

```json
[
  { "file": "low_dark_01.mp4", "energy": 0.2, "brightness": 0.2 },
  { "file": "high_bright_01.mp4", "energy": 0.8, "brightness": 0.8 }
]
```

無音、入力停止・切断中は候補を保持します。`clips.json`、MP4、FFmpeg に問題がある場合は、原因を画面に表示したまま操作できる状態を保ちます。
