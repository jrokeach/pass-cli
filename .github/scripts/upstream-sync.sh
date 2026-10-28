#!/usr/bin/env bash
# Keeps the fork's patch stack (RELEASE_BRANCH) rebased on upstream. Used by upstream-sync.yml.
#
#   upstream-sync.sh sync
#       Mirror upstream's branch to MIRROR_BRANCH and its tags, rebase RELEASE_BRANCH's patches
#       onto upstream's tip and open a PR from fork/sync/<sha>. If the rebase stops on a conflict,
#       the PR is a draft whose branch holds upstream's tip, with the steps to resolve it.
#
#   upstream-sync.sh finalize <branch> <sha>
#       Run once Fork CI has passed on <sha>, the head of a fork/sync/* branch. Moves
#       RELEASE_BRANCH to <sha> (GitHub then marks the PR merged) and tags a fork release
#       v<version>-<TAG_SUFFIX>.1 for each upstream tag the sync brought in. Pushing a tag runs
#       Fork CI, which builds and publishes the release.
#
# Pushes need a token that can push workflow files, since the patch stack and upstream both
# change .github/workflows; the GITHUB_TOKEN can't, and its pushes don't trigger other workflows.
set -euo pipefail

: "${UPSTREAM_REPO:?set UPSTREAM_REPO to owner/repo of upstream}"
: "${GITHUB_REPOSITORY:?}"
UPSTREAM_BRANCH=${UPSTREAM_BRANCH:-main}
MIRROR_BRANCH=${MIRROR_BRANCH:-main}
RELEASE_BRANCH=${RELEASE_BRANCH:-fork/release}
TAG_SUFFIX=${TAG_SUFFIX:-mailbox}
SYNC_PREFIX=fork/sync/
LABEL=upstream-sync
CONFLICT_LABEL=upstream-sync-conflict
GH=(gh --repo "$GITHUB_REPOSITORY")

git config user.name 'github-actions[bot]'
git config user.email '41898282+github-actions[bot]@users.noreply.github.com'

# Rebases happen in a separate worktree so this script's own checkout is never switched under it.
work=$(mktemp -d)
git worktree add --quiet --detach "$work" HEAD
trap 'git worktree remove --force "$work"' EXIT

fetch() {
  git remote get-url upstream >/dev/null 2>&1 ||
    git remote add upstream "https://github.com/$UPSTREAM_REPO.git"
  # Upstream tags go to their own namespace so they never clash with the fork's tags.
  git fetch --quiet --no-tags upstream \
    "+refs/heads/$UPSTREAM_BRANCH:refs/remotes/upstream/$UPSTREAM_BRANCH" \
    "+refs/tags/*:refs/upstream-tags/*"
  git fetch --quiet --prune --prune-tags --tags origin "+refs/heads/*:refs/remotes/origin/*"
}

short() { git rev-parse --short "$1"; }

# Upstream tags reachable from $1 but not from $2, oldest first.
new_upstream_tags() {
  git for-each-ref --sort=creatordate --format='%(refname:strip=2)' \
    --merged "$1" --no-merged "$2" refs/upstream-tags
}

fork_tag() { echo "v${1#v}-$TAG_SUFFIX.1"; }

# Reads a value the sync step left in a PR body as <!-- upstream-sync-<key>: <sha> -->.
field() { grep -o "upstream-sync-$1: [0-9a-f]\{40\}" <<<"$2" | head -n1 | cut -d' ' -f2 || true; }

# Rebases the patches in <base>..<tip> onto <onto> in the worktree. Leaves a stopped rebase
# in place on failure so the caller can read what conflicted.
rebase_patches() {
  git -C "$work" checkout --quiet --detach "$2"
  git -C "$work" rebase --quiet --onto "$3" "$1" >/dev/null 2>&1
}

mirror_upstream() {
  local target=$1
  if git merge-base --is-ancestor "origin/$MIRROR_BRANCH" "$target"; then
    if [[ $(git rev-parse "origin/$MIRROR_BRANCH") != "$target" ]]; then
      git push --quiet origin "$target:refs/heads/$MIRROR_BRANCH"
      echo "Fast-forwarded $MIRROR_BRANCH to upstream $(short "$target")"
    fi
  else
    echo "::warning::$MIRROR_BRANCH has commits upstream doesn't have; not mirroring it"
  fi

  local tag refspecs=()
  for tag in $(git for-each-ref --format='%(refname:strip=2)' refs/upstream-tags); do
    git rev-parse --quiet --verify "refs/tags/$tag" >/dev/null ||
      refspecs+=("refs/upstream-tags/$tag:refs/tags/$tag")
  done
  if ((${#refspecs[@]})); then
    git push --quiet origin "${refspecs[@]}"
    git fetch --quiet --tags origin
    echo "Mirrored ${#refspecs[@]} upstream tags"
  fi
}

cmd_sync() {
  fetch
  local target base release
  target=$(git rev-parse "upstream/$UPSTREAM_BRANCH")
  release=$(git rev-parse "origin/$RELEASE_BRANCH")
  mirror_upstream "$target"

  base=$(git merge-base "$release" "$target") || {
    echo "::error::$RELEASE_BRANCH shares no history with upstream"
    exit 1
  }
  if [[ $base == "$target" ]]; then
    echo "$RELEASE_BRANCH is already based on upstream $(short "$target")"
    return
  fi

  # One sync PR at a time: keep an open one that is for this exact rebase, close the rest.
  local prs number body
  prs=$("${GH[@]}" pr list --state open --label "$LABEL" --json number,body,labels \
    --jq '.[] | @base64')
  for pr in $prs; do
    number=$(base64 -d <<<"$pr" | jq -r .number)
    body=$(base64 -d <<<"$pr" | jq -r .body)
    if [[ $(field target "$body") == "$target" && $(field release "$body") == "$release" ]]; then
      echo "PR #$number already syncs $RELEASE_BRANCH onto upstream $(short "$target")"
      return
    fi
    local delete=(--delete-branch)
    # Keep the branch of a conflict PR: it may hold someone's partial resolution.
    if base64 -d <<<"$pr" | jq -e --arg l "$CONFLICT_LABEL" 'any(.labels[]; .name == $l)' >/dev/null; then
      delete=()
    fi
    "${GH[@]}" pr close "$number" "${delete[@]}" \
      --comment "Superseded: upstream is now at $(short "$target") / $RELEASE_BRANCH at $(short "$release")."
  done

  local branch=$SYNC_PREFIX$(short "$target") head conflict=
  local stopped='' files=''
  if rebase_patches "$base" "$release" "$target"; then
    head=$(git -C "$work" rev-parse HEAD)
    if [[ $head == "$target" ]]; then
      echo "::warning::Every fork patch is already upstream; nothing to rebase. Update $RELEASE_BRANCH by hand."
      return
    fi
  else
    conflict=1
    stopped=$(git -C "$work" rev-parse --quiet --verify REBASE_HEAD || true)
    files=$(git -C "$work" diff --name-only --diff-filter=U || true)
    git -C "$work" rebase --abort
    head=$target
  fi
  git push --quiet --force origin "$head:refs/heads/$branch"

  local tags title upstream_desc
  tags=$(new_upstream_tags "$target" "$base")
  upstream_desc=$(git for-each-ref --points-at "$target" --format='%(refname:strip=2)' refs/upstream-tags | tail -n1)
  upstream_desc=${upstream_desc:-$(short "$target")}
  title="Rebase $RELEASE_BRANCH onto upstream $upstream_desc"

  local bodyfile
  bodyfile=$(mktemp)
  {
    echo "Upstream [$UPSTREAM_REPO](https://github.com/$UPSTREAM_REPO) moved from \`$(short "$base")\` to \`$(short "$target")\`."
    echo
    if [[ -n $conflict ]]; then
      echo "## Conflict"
      echo
      echo "Rebasing the fork patches stopped at \`$(git log -1 --format='%h %s' "${stopped:-$base}")\`:"
      echo
      sed 's/^/- `/; s/$/`/' <<<"$files"
      echo
      echo "This branch holds upstream's tip without the fork patches. To resolve:"
      echo
      echo '```bash'
      echo "git fetch origin"
      echo "git checkout -B $branch origin/$RELEASE_BRANCH"
      echo "git rebase --onto $target $base"
      echo "# fix the conflicts, then: git add <files> && git rebase --continue"
      echo "git push --force origin $branch"
      echo '```'
      echo
      echo "Once Fork CI passes on the pushed branch, the Upstream sync workflow moves \`$RELEASE_BRANCH\` to it and tags the release. Don't merge this PR with the merge button."
    else
      echo "The fork patches rebased cleanly. Once Fork CI passes on this branch, the Upstream sync workflow moves \`$RELEASE_BRANCH\` to it (marking this PR merged) and tags the release."
    fi
    echo
    if [[ -n $tags ]]; then
      echo "## Upstream tags → fork releases"
      echo
      local t
      for t in $tags; do echo "- \`$t\` → \`$(fork_tag "$t")\`"; done
      echo
    fi
    echo "## Upstream commits"
    echo
    git log --no-merges --format='- `%h` %s' "$base..$target" | head -n 100
    echo
    if [[ -z $conflict ]]; then
      echo "## Fork patches"
      echo
      git log --reverse --no-merges --format='- `%h` %s' "$target..$head"
      echo
    fi
    echo "<!-- upstream-sync-target: $target -->"
    echo "<!-- upstream-sync-base: $base -->"
    echo "<!-- upstream-sync-release: $release -->"
  } >"$bodyfile"

  "${GH[@]}" label create "$LABEL" --color 1d76db --force \
    --description "Automated rebase of the fork onto upstream" >/dev/null
  local args=(--base "$RELEASE_BRANCH" --head "$branch" --body-file "$bodyfile" --label "$LABEL")
  if [[ -n $conflict ]]; then
    "${GH[@]}" label create "$CONFLICT_LABEL" --color d93f0b --force \
      --description "Upstream sync needs a manual conflict resolution" >/dev/null
    args+=(--draft --label "$CONFLICT_LABEL" --title "$title (conflicts)")
  else
    args+=(--title "$title")
  fi
  "${GH[@]}" pr create "${args[@]}"
  if [[ -n $conflict ]]; then
    echo "::warning::Rebase onto upstream $(short "$target") conflicts; opened a draft PR"
  fi
}

cmd_finalize() {
  local branch=${1:?branch} sha=${2:?sha}
  [[ $branch == "$SYNC_PREFIX"* ]] || { echo "Not a sync branch: $branch"; return; }
  fetch

  local pr number body target base release
  pr=$("${GH[@]}" pr list --state open --label "$LABEL" --head "$branch" \
    --json number,body,headRefOid --jq '.[0] // empty')
  [[ -n $pr ]] || { echo "No open sync PR for $branch"; return; }
  number=$(jq -r .number <<<"$pr")
  body=$(jq -r .body <<<"$pr")
  target=$(field target "$body")
  base=$(field base "$body")
  release=$(field release "$body")
  [[ -n $target && -n $base && -n $release ]] || { echo "::error::PR #$number has no sync metadata"; exit 1; }

  if [[ $(jq -r .headRefOid <<<"$pr") != "$sha" ]]; then
    echo "$branch has moved past $(short "$sha"); waiting for CI on its new head"
    return
  fi
  if [[ $sha == "$target" ]] || ! git merge-base --is-ancestor "$target" "$sha"; then
    echo "$branch is not upstream $(short "$target") plus the fork patches yet (unresolved conflict?)"
    return
  fi
  if [[ $(git rev-parse "origin/$RELEASE_BRANCH") != "$release" ]]; then
    "${GH[@]}" pr comment "$number" --body "\`$RELEASE_BRANCH\` changed since this PR was opened, so it was not updated. The next sync run replaces this PR."
    return
  fi

  git push --quiet --force-with-lease="refs/heads/$RELEASE_BRANCH:$release" origin "$sha:refs/heads/$RELEASE_BRANCH"
  echo "Moved $RELEASE_BRANCH to $(short "$sha")"

  local t tag commit tagged=()
  for t in $(new_upstream_tags "$target" "$base"); do
    tag=$(fork_tag "$t")
    if git rev-parse --quiet --verify "refs/tags/$tag" >/dev/null; then
      echo "$tag already exists"
      continue
    fi
    commit=$(git rev-parse "refs/upstream-tags/$t^{commit}")
    if [[ $commit == "$target" ]]; then
      git tag "$tag" "$sha"
    elif rebase_patches "$base" "$release" "$commit"; then
      # An upstream release older than the tip: tag the fork patches on top of it.
      git tag "$tag" "$(git -C "$work" rev-parse HEAD)"
    else
      git -C "$work" rebase --abort
      echo "::warning::Fork patches don't apply cleanly to upstream $t; not tagging $tag"
      continue
    fi
    # One push per tag: GitHub doesn't run workflows for pushes of more than three tags.
    git push --quiet origin "refs/tags/$tag"
    tagged+=("$tag")
    echo "Tagged $tag"
  done

  local state=
  for _ in 1 2 3 4 5 6; do
    state=$("${GH[@]}" pr view "$number" --json state --jq .state)
    [[ $state == MERGED ]] && break
    sleep 5
  done
  local note="\`$RELEASE_BRANCH\` now points at $(short "$sha")."
  if ((${#tagged[@]})); then
    note+=" Tagged ${tagged[*]}; Fork CI builds and publishes the release."
  fi
  if [[ $state == MERGED ]]; then
    "${GH[@]}" pr comment "$number" --body "$note"
  else
    "${GH[@]}" pr close "$number" --comment "$note"
  fi
  git push --quiet origin --delete "$branch" || true
}

case ${1:-} in
  sync) cmd_sync ;;
  finalize) shift; cmd_finalize "$@" ;;
  *) echo "usage: $0 sync | finalize <branch> <sha>" >&2; exit 2 ;;
esac
