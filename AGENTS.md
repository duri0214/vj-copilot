# vj-copilot 開発ルール

このリポジトリで作業するときは、作業内容に関係する `.codex/rules/` と `.codex/skills/` を確認する。

## 基本ルール

- Rust の既存構成と標準ツールを優先し、必要な範囲だけ変更する。
- 開発作業は、対応する GitHub Issue を用意してから Issue 番号付きのトピックブランチで行う。
- `main` や `master` に直接コミットしない。
- Rust のコード変更では、対象範囲に応じて `cargo fmt`、`cargo test`、`cargo clippy` を実行する。
- GitHub の Issue、Project、PR、レビューコメントの操作には `gh` CLI を使う。
- ルールやスキル自体を変更するときも、通常の Issue、ブランチ、確認、コミット、PR の流れに従う。

## ルールの使い分け

- 常に適用する設計・運用方針: `.codex/rules/principles.md`、`.codex/rules/project.md`、`.codex/rules/ddd.md`
- Rust のコード: `.codex/rules/rust.md`
- Rust のテスト: `.codex/rules/testing.md`

## スキルの使い分け

- Issue 作成: `ticket`
- ブランチ作成・着手: `branch`
- コミット・push: `commit`
- PR 作成: `pull-request`
- PR レビュー・指摘対応: `review`
- マージ後の整理: `cleanup-branch`
