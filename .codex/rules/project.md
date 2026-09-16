---
apply: always
---

# プロジェクト共通ルール

## Git / GitHub

- ファイルの変更はローカル作業ツリーで行い、`git add`、`git commit`、`git push`、PR の通常フローで反映する。
- GitHub API、Contents API、MCP の更新系操作でリモートブランチのファイルを直接変更しない。
- `main` / `master` へ直接コミットしない。
- `git commit --amend`、`git push --force`、`git push --force-with-lease` は、明示的な許可なしに使わない。
- Issue、Project、PR、レビューコメントは `gh` CLI で操作する。
- `git` や `gh` が失敗したときは、APIなどで迂回せず、原因を確認してから続行する。

## 変更範囲

- Issue の要件と関係するファイルだけを変更する。
- 既存の動作を変える場合は、利用者が判断できるエラーとログを残す。
- 外部サービスの詳細や秘密情報を利用者向けのエラーに含めない。必要な診断情報は適切なログへ記録する。
- Rust の設計方針は `ddd.md`、テストの方針は `testing.md`、言語固有の方針は `rust.md` に従う。

## Issue / PR メタ情報

- 対象リポジトリに存在するラベルと Project だけを使う。別リポジトリ固有の `app:` ラベルを持ち込まない。
- Issue 番号をブランチ名の先頭に付け、PR から対応 Issue を追跡できるようにする。
- 変更が検証できない場合は、実行できなかったコマンドと理由を Issue または PR の報告に残す。
