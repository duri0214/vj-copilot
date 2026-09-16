---
name: pull-request
description: GitHub CLI で Rust リポジトリの PR を日本語で作成する。Issue 番号、差分、検証結果を本文へ反映する。
---

# プルリクエスト作成

## 事前確認

1. `git status --short --branch` で現在のブランチと作業ツリーを確認する。
2. `gh repo view --json defaultBranchRef --jq .defaultBranchRef.name` で base ブランチを取得する。`main` / `master` 上では PR を作成しない。
3. ブランチ名の先頭の数字を Issue 番号として扱い、`gh issue view <番号> --json title,body,labels,assignees,projectItems,url` で Issue を確認する。
4. `git diff --stat origin/<base>..HEAD`、`git diff --name-status origin/<base>..HEAD`、必要に応じて `git log --oneline origin/<base>..HEAD` を確認する。
5. 未コミット変更がある場合は `commit` スキルの確認、コミット、push を済ませる。
6. Rust の変更では、実行済みの `cargo fmt`、`cargo test`、`cargo clippy` と確認範囲を整理する。ルール・ドキュメントだけなら `git diff --check` を記載する。

## PR 本文

本文は日本語で、次の形式にする。

```text
## 概要
[変更内容]

## 主な変更点
- [変更点]

## 目検による動作確認手順
- [ ] [確認操作と期待値]

## 自動テストでカバーできた範囲
[実行したコマンドと結果]

## 関連 Issue
Closes #<Issue番号>
<Issue URL>
```

- 目検手順は、操作と期待値が分かるチェックボックスで書く。
- Issue 番号がある場合は、URLだけでなく必ず `Closes #<Issue番号>` を含める。
- 別リポジトリの Issue を閉じる場合だけ `Closes <owner>/<repo>#<番号>` を使う。

## 作成

```bash
gh pr create --base <base> --head <current-branch> \
  --title "#<Issue番号> <Issueタイトル>" --body-file <body-file>
```

作成後、`gh pr view --json url,number,title` で URL を確認する。Issue のラベル、assignee、Project を確認し、同じ設定を適用できる範囲で PR にも設定する。設定に失敗しても PR 自体は残し、失敗理由を報告する。

*** Add File: C:\Users\yoshi\OneDrive\dev\vj-copilot\.codex\skills\cleanup-branch\SKILL.md
---
name: cleanup-branch
description: マージ済み PR の後に、main または master へ戻り、マージ済みのローカル Issue ブランチを安全に整理する。
---

# マージ後ブランチ整理

1. `git status --short --branch` で現在ブランチと未コミット変更を確認する。
2. 未コミット変更がある場合は `git stash push -u -m "cleanup-branch:<元のブランチ名>"` で退避する。stash に失敗したら切り替えない。
3. `gh repo view --json defaultBranchRef --jq .defaultBranchRef.name` で既定ブランチを確認する。
4. `git fetch origin --prune` を実行する。fetch に失敗したらブランチ削除を続けない。
5. 現在ブランチに対応する PR がある場合は、`gh pr view --json state,mergedAt,baseRefName,headRefName` で merged を確認する。未マージや判定不能のブランチは削除しない。
6. 既定ブランチへ `git switch <default-branch>` で切り替え、`git pull --ff-only origin <default-branch>` で最新化する。失敗したら削除しない。
7. `git branch --merged <default-branch>` と `gh pr list --head <branch> --state merged` で削除対象を確認する。
8. マージ済みの Issue ブランチだけ `git branch -d <branch>` で削除する。通常の未マージブランチに `-D` は使わない。
9. `git ls-remote --heads origin <branch>` でリモートに残っているか確認する。リモートブランチの削除は対象を一覧で提示し、ユーザーの明示的な了承後に `git push origin --delete <branch>` を実行する。
10. `git status --short --branch`、`git branch --list`、`git branch --remotes` で整理後の状態を確認する。

stash を作成した場合は、元のブランチへ戻して復元するか、既定ブランチに残すかをユーザーへ確認してから処理する。ローカルの追跡 ref の prune と GitHub 上のブランチ削除を混同しない。
