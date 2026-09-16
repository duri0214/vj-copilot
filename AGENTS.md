# vj-copilot 開発ルール

このリポジトリで作業するときは、作業内容に関係する `.codex/rules/` と `.codex/skills/` を確認する。

## 基本ルール

- Rust の既存構成と標準ツールを優先し、必要な範囲だけ変更する。
- 開発作業は、対応する GitHub Issue を用意してから Issue 番号付きのトピックブランチで行う。
- `main` や `master` に直接コミットしない。
- Rust のコード変更では、対象範囲に応じて `cargo fmt`、`cargo test`、`cargo clippy` を実行する。
- GitHub の Issue、Project、PR、レビューコメントの操作には `gh` CLI を使う。
- ルールやスキル自体を変更するときも、通常の Issue、ブランチ、確認、コミット、PR の流れに従う。
- コード、ルール、ドキュメントを変更する依頼では、確認後に commit、push、PR作成または更新まで進める。ユーザーが `commit不要`、`push不要`、`PR不要`、`まだコミットしない` と明示した場合だけ停止する。
- 説明、調査、レビュー結果の報告、状態確認だけを求められた場合は、変更や commit、push、PR作成を行わない。

## ルールの使い分け

- 常に適用する設計・運用方針: `.codex/rules/principles.md`、`.codex/rules/project.md`、`.codex/rules/ddd.md`
- Rust のコード: `.codex/rules/rust.md`
- Rust のテスト: `.codex/rules/testing.md`

## スキルの使い分け

- Issue 作成: `ticket`
- PR レビュー・指摘対応: `review`
- マージ後の整理: `cleanup-branch`

ブランチ作成、コミット、push、PR 作成は、この `AGENTS.md` の常用フローに従う。個別スキルへ重複して定義しない。

## ブランチ、コミット、PR の常用フロー

### ブランチ作成

1. 作業開始前に対応する GitHub Issue を用意する。Issue がなければ `ticket` スキルで作成する。
2. `git status --short --branch` で現在のブランチと変更を確認する。
3. 未コミット変更がある場合は、`git stash push -u -m "branch:<Issue番号>"` で退避する。既存作業を壊す可能性がある変更は、復元先を判断するまで stash に残す。
4. `gh repo view --json defaultBranchRef --jq .defaultBranchRef.name` で既定ブランチを確認し、`git fetch origin --prune` で最新化する。
5. 既定ブランチを起点に、`<Issue番号>-<英小文字と数字の短い説明>` 形式のトピックブランチを作成する。例: `1-port-rust-codex-rules-skills`。
6. Issue の assignee、Project、Project status を設定できる範囲で更新し、着手後は `In progress` にする。

### コミットと push

1. `main` / `master` 上ではコミットしない。対応する Issue 番号付きブランチを使う。
2. `AGENTS.md` と変更に関係する `.codex/rules/` を確認する。
3. Rust プロジェクトでは、リポジトリの手順を優先して次を実行する。

   ```bash
   cargo fmt --all -- --check
   cargo test --all-targets
   cargo clippy --all-targets --all-features -- -D warnings
   ```

   `Cargo.toml` がないルール・ドキュメントだけの変更では、`git diff --check` を実行する。
4. `git diff --stat`、`git diff --check`、`git diff` で Issue に関係する差分だけであることを確認する。
5. 問題がなければ、内容が分かる短いメッセージでコミットし、push する。変更依頼に対しては、明示的な停止指定がない限り確認待ちで止めない。
6. `git commit --amend`、`git push --force`、`git push --force-with-lease` は、明示的な許可なしに使わない。

### PR 作成

1. `main` / `master` 上ではPRを作成しない。baseブランチは `gh repo view --json defaultBranchRef --jq .defaultBranchRef.name` で確認する。
2. ブランチ名の先頭の数字を Issue 番号として扱い、Issue のタイトル、本文、ラベル、assignee、Project を確認する。
3. `git diff --stat origin/<base>..HEAD`、`git diff --name-status origin/<base>..HEAD` でPRの差分を確認する。
4. PR本文は日本語で、概要、主な変更点、目検手順、自動テストの範囲、`Closes #<Issue番号>` を含める。
5. 目検手順は操作と期待値を `- [ ]` 形式で書く。実行していない確認を `[x]` にしない。
6. `gh pr create --base <base> --head <current-branch> --title "#<Issue番号> <Issueタイトル>" --body-file <body-file>` で作成し、作成後に URL、assignee、ラベル、Project を確認する。変更依頼に対しては、明示的な `PR不要` 指定がない限り作成または更新まで進める。

git や gh の操作が失敗した場合は、API で迂回せず原因を切り分けて報告する。Project 操作の権限が不足する場合は、`gh auth refresh -s read:project -s project` が必要であることを伝える。
