---
name: commit
description: Rust リポジトリでコミットと push を行う前に、ルール確認・フォーマット・テスト・差分確認を実行する。
---

# コミット・push

1. `git status --short --branch` でブランチと変更を確認する。
2. `main` / `master` 上ではコミットしない。Issue 番号付きのトピックブランチへ切り替える。
3. 対応する PR がある場合は、`gh pr view --json state,mergedAt,baseRefName` でマージ済みでないことを確認する。
4. `AGENTS.md` と変更に関係する `.codex/rules/` を読み、今回の差分がルールに沿っているか確認する。
5. Rust プロジェクトの場合、変更範囲に応じて次を実行する。

   ```bash
   cargo fmt --all -- --check
   cargo test --all-targets
   cargo clippy --all-targets --all-features -- -D warnings
   ```

   リポジトリの README や CI に別のコマンドが定義されている場合は、そちらを優先する。ドキュメント・ルールだけの変更で `Cargo.toml` がない場合は、`git diff --check` を最低限実行し、存在しない Rust コマンドを無理に実行しない。
6. `git diff --stat`、`git diff --check`、`git diff` で今回の Issue に関係する差分だけであることを確認する。
7. 問題がなければ、変更内容が分かる日本語または英語の短いメッセージでコミットする。Issue 番号が分かる場合は必要に応じて含める。
8. ユーザーから push 不要の指示がなければ、現在のブランチを push する。
9. push 後に PR 作成まで進める場合は `pull-request` スキルへ引き継ぐ。

## 失敗時

- フォーマット、テスト、clippy の失敗は原因を確認し、今回の変更に起因するものだけ修正して再実行する。
- 既存の無関係な失敗は、コマンドと理由を報告してから次の操作を判断する。
- `git commit` や `git push` が失敗した場合は、API で迂回せず原因を報告する。
- force push や履歴の上書きは行わない。
