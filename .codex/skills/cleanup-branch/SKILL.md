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
