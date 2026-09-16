---
name: branch
description: Issue に対応する Rust リポジトリのトピックブランチを作成し、Project の着手状態を更新する。Issue 作成は ticket スキルへ引き継ぐ。
---

# ブランチ運用

## 基本方針

- 開発作業には、作業開始前に対応する GitHub Issue を用意する。
- `main` / `master` へ直接コミットせず、Issue 番号付きブランチで作業する。
- Issue がない場合は、先に `ticket` スキルで作成してからブランチを作る。
- ブランチ作成後、Issue の assignee、Project、Project status を設定できる範囲で更新する。

## 手順

1. `git status --short --branch` で現在のブランチと変更を確認する。
2. 作業ツリーに未コミット変更がある場合は、`git stash push -u -m "branch-skill:<Issue番号>"` で退避する。新しいブランチへ戻すべきか判断できない変更は、stash 名を報告する。
3. `gh repo view --json defaultBranchRef --jq .defaultBranchRef.name` で既定ブランチを確認する。
4. `git fetch origin --prune` を実行し、既定ブランチから新しいブランチを作る。
5. ブランチ名は `<Issue番号>-<英小文字と数字の短い説明>` とする。例: `1-port-rust-codex-rules-skills`。
6. `git switch -c <ブランチ名> origin/<既定ブランチ>` で切り替える。
7. Issue の assignee を自分に設定し、Project に登録されていなければ追加する。
8. Project の `Status` を `In progress` に更新する。親 Issue があり、親の status が `To do` の場合は親も更新する。
9. `git status --short --branch` で作業先を確認する。

## Project 更新例

```bash
gh project item-edit <project-number> --owner <owner> \
  --url <issue-url> --field Status --value "In progress"
```

Project の権限が不足する場合は、`gh auth refresh -s read:project -s project` が必要であることを報告する。GitHub 操作の失敗時に API で迂回しない。

## 確認が必要な場合

- 対応 Issue がないのに、Issue なしで進める明示的な指示もない場合
- 複数の Issue や Project が候補になり、対象を一意に決められない場合
- stash した変更を戻すと既存作業を壊す可能性がある場合
- git または gh の操作が失敗し、原因が特定できない場合
