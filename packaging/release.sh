#!/usr/bin/env bash
# Cut a release: build what this machine can build, then publish it.
#
#   packaging/release.sh --build            build artefacts only
#   packaging/release.sh --tag              create and push the git tag
#   packaging/release.sh --publish          upload to GitLab and GitHub
#   packaging/release.sh --all              all three, in that order
#   packaging/release.sh --manifest         re-read the release and republish
#                                           SHA256SUMS and `latest`
#
# Run --manifest LAST, after the macOS and Windows workflows have attached
# their installers. Those are pressed by hand on GitHub and land after the
# release exists, so a checksum file written during --publish is missing
# exactly the two artefacts people most need to verify.
#
# Tokens come from the environment and are never read from or written to a
# file, so nothing here can leave a credential behind in the checkout:
#
#   GITLAB_TOKEN   needs api scope (Release: Create, Package: Create)
#   GITHUB_TOKEN   needs repo scope, or use an already authenticated `gh`
#
# Set them for one command rather than exporting them into your shell history:
#
#   GITLAB_TOKEN=... GITHUB_TOKEN=... packaging/release.sh --all
#
# The version is read from the workspace Cargo.toml. There is no flag to
# override it: two sources of truth for a version number is how a release ends
# up labelled one thing and containing another.

set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd "$here/.." && pwd)"
dist="$root/target/release-artifacts"

version="$(sed -n 's/^version = "\(.*\)"$/\1/p' "$root/Cargo.toml" | head -1)"
[ -n "$version" ] || { echo "could not read version from Cargo.toml" >&2; exit 1; }
tag="v$version"

gitlab_project="alterion-software%2Falterion-open-project"
github_repo="Alterion-Software/Alterion-Open-Project"

step() { printf '\n\033[36m==>\033[0m %s\n' "$1"; }
warn() { printf '\033[33mwarning:\033[0m %s\n' "$1" >&2; }
die()  { printf '\033[31merror:\033[0m %s\n' "$1" >&2; exit 1; }

do_build=0 do_tag=0 do_publish=0 do_manifest=0
for arg in "$@"; do
  case "$arg" in
    --build)   do_build=1 ;;
    --tag)     do_tag=1 ;;
    --publish) do_publish=1 ;;
    --all)     do_build=1; do_tag=1; do_publish=1 ;;
    --manifest) do_manifest=1 ;;
    -h|--help) sed -n '2,24p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *)         die "unknown option: $arg" ;;
  esac
done
[ $((do_build + do_tag + do_publish + do_manifest)) -gt 0 ] || die "nothing to do; try --all or --help"

# ---------------------------------------------------------------- build

if [ "$do_build" = 1 ]; then
  step "Checking the tree is releasable"
  # A release built from a dirty tree cannot be reproduced from its tag, and
  # the tag is the only thing anyone downloading it will have.
  [ -z "$(git -C "$root" status --porcelain)" ] || die "uncommitted changes; commit them first"

  cargo test --workspace --manifest-path "$root/Cargo.toml"
  cargo clippy --workspace --all-targets --manifest-path "$root/Cargo.toml" -- -D warnings

  mkdir -p "$dist"

  step "Linux binary"
  cargo build --release -p aop-app --manifest-path "$root/Cargo.toml"
  linux="$dist/alterion-open-project-$version-x86_64-linux.tar.gz"
  tar -czf "$linux" \
      -C "$root/target/release" alterion-open-project \
      -C "$root/packaging/linux" alterion-open-project.desktop \
                                 alterion-open-project.svg \
                                 alterion-open-project.xml
  echo "  $linux"

  step "Windows installer"
  # Cross-builds from Linux, so it runs here. It needs the mingw toolchain and
  # the msvc target; if either is missing, say so rather than shipping a
  # release that quietly has no Windows download.
  if "$root/packaging/windows/build-installer.sh"; then
    cp "$root/packaging/windows/dist"/*.exe "$dist/"
    echo "  $(ls "$dist"/*.exe)"
  else
    warn "Windows installer failed; the release will have no Windows download"
  fi

  step "macOS disk image"
  # Apple's SDK cannot be redistributed, so this is the one artefact that has
  # to be built on a Mac. On Linux it is skipped, not failed.
  if [ "$(uname -s)" = "Darwin" ]; then
    "$root/packaging/macos/build-dmg.sh"
    cp "$root/packaging/macos/dist"/*.dmg "$dist/"
  else
    warn "not on macOS, skipping the .dmg; build it there with packaging/macos/build-dmg.sh and add it to the release"
  fi

  step "Checksums and the version marker"
  # `latest` is the only file a running app fetches on a routine check, so it
  # holds the version and nothing else. SHA256SUMS is the manifest: which
  # platforms have a build, what each is called, and what it must hash to.
  # A platform with no line here has no update, which is the truthful answer
  # and better than an app guessing a filename and finding a 404.
  printf '%s\n' "$version" > "$dist/latest"
  checksum_dir "$dist"
  cat "$dist/SHA256SUMS"
fi

# ---------------------------------------------------------------- tag

if [ "$do_tag" = 1 ]; then
  step "Tagging $tag"
  if git -C "$root" rev-parse -q --verify "refs/tags/$tag" >/dev/null; then
    warn "$tag already exists locally, leaving it alone"
  else
    # An annotated tag carries a message and an author; a lightweight one is
    # just a moving pointer, which is not what a release should hang off.
    git -C "$root" tag -a "$tag" -m "Alterion Open Project $version"
  fi
  git -C "$root" push origin "$tag"
fi

# The two descriptor files also go to a FIXED slot, under the literal version
# "latest". That slot is the only address a running app has to know, and it is
# deliberately not a release permalink:
#
#   GitHub's /releases/latest/ skips anything marked prerelease, so it is
#   only usable while every release is marked latest, which is one checkbox
#   away from breaking. GitLab's release permalink needs direct asset links
#   with a filepath, which the generic package registry does not produce.
#
# The generic registry accepts any string as a version, so "latest" is a
# perfectly good permanent address that no release semantics can move.
publish_descriptors_to_fixed_slot() {
  local dir="$1"
  [ -n "${GITLAB_TOKEN:-}" ] || { warn "GITLAB_TOKEN not set, the fixed 'latest' slot was not updated"; return; }
  local api="https://gitlab.com/api/v4/projects/$gitlab_project"
  local f
  for f in latest SHA256SUMS; do
    curl --fail --silent --show-error \
         --header "PRIVATE-TOKEN: $GITLAB_TOKEN" \
         --upload-file "$dir/$f" \
         "$api/packages/generic/alterion-open-project/latest/$f" >/dev/null
    echo "  fixed slot updated: $f"
  done
}

# ---------------------------------------------------------------- publish

# The release notes are the section of the changelog for this version, so the
# two can never drift apart.
notes_for() {
  awk -v want="## $version" '
    $0 == want { on = 1; next }
    on && /^## / { exit }
    on { print }
  ' "$root/CHANGELOG.md"
}

if [ "$do_publish" = 1 ]; then
  [ -d "$dist" ] || die "no artefacts in $dist; run --build first"
  notes="$(notes_for)"
  [ -n "$notes" ] || die "no '## $version' section in CHANGELOG.md"

  step "GitLab"
  if [ -n "${GITLAB_TOKEN:-}" ]; then
    api="https://gitlab.com/api/v4/projects/$gitlab_project"
    links=()
    for f in "$dist"/*; do
      base="$(basename "$f")"
      # The generic package registry is what a release asset links to; the
      # release itself only holds links, never bytes.
      curl --fail --silent --show-error \
           --header "PRIVATE-TOKEN: $GITLAB_TOKEN" \
           --upload-file "$f" \
           "$api/packages/generic/alterion-open-project/$version/$base" >/dev/null
      links+=("{\"name\":\"$base\",\"url\":\"$api/packages/generic/alterion-open-project/$version/$base\"}")
      echo "  uploaded $base"
    done
    assets="$(IFS=,; echo "${links[*]}")"
    jq -n --arg tag "$tag" --arg name "Alterion Open Project $version" \
          --arg desc "$notes" --argjson links "[$assets]" \
          '{tag_name:$tag, name:$name, description:$desc, assets:{links:$links}}' \
    | curl --fail --silent --show-error \
           --header "PRIVATE-TOKEN: $GITLAB_TOKEN" \
           --header "Content-Type: application/json" \
           --data @- "$api/releases" >/dev/null
    echo "  release created"
    publish_descriptors_to_fixed_slot "$dist"
  else
    warn "GITLAB_TOKEN not set, skipping GitLab"
  fi

  step "GitHub"
  if command -v gh >/dev/null && { [ -n "${GITHUB_TOKEN:-}" ] || gh auth status >/dev/null 2>&1; }; then
    # Published as the latest release, not a prerelease, even though the
    # version carries a -beta suffix. Marking it prerelease hides it from
    # GitHub's /releases/latest/ and from the repository header, which is the
    # opposite of what a beta you want people to install should do.
    gh release create "$tag" "$dist"/* \
       --repo "$github_repo" \
       --title "Alterion Open Project $version" \
       --notes "$notes" \
       --latest
    echo "  release created"
  else
    warn "no authenticated gh and no GITHUB_TOKEN, skipping GitHub"
  fi
fi

# Hash everything in a directory except the two files that describe it. `*`
# is expanded before the redirection truncates SHA256SUMS, so hashing `*`
# blind would list a zero-length SHA256SUMS as though it were an artefact.
checksum_dir() {
  local dir="$1"
  ( cd "$dir" && rm -f SHA256SUMS && \
    find . -maxdepth 1 -type f ! -name SHA256SUMS ! -name latest -printf '%P\n' \
    | sort | xargs -r sha256sum -- > SHA256SUMS )
}

# ---------------------------------------------------------------- manifest

# Re-read what is actually attached to the release and republish the two files
# that describe it. Run after the macOS and Windows workflows have attached
# their installers: those are pressed by hand and land after the release
# exists, so the checksums written during --publish cannot include them.
#
# The hashes are computed from the downloaded files, never copied from
# anywhere. A checksum that was not derived from the bytes it describes is
# decoration.
if [ "$do_manifest" = 1 ]; then
  command -v gh >/dev/null || die "the manifest is rebuilt from the GitHub release; gh is needed"
  work="$(mktemp -d)"
  trap 'rm -rf "$work"' EXIT

  step "Reading what is attached to $tag"
  gh release download "$tag" --repo "$github_repo" --dir "$work" \
     --pattern '*' --clobber
  # The old descriptors are inputs to nothing; drop them before rehashing.
  rm -f "$work/SHA256SUMS" "$work/latest"
  ls -1 "$work" | sed 's/^/  /'
  [ -n "$(ls -A "$work")" ] || die "no artefacts on that release"

  step "Rebuilding SHA256SUMS and latest"
  printf '%s\n' "$version" > "$work/latest"
  checksum_dir "$work"
  cat "$work/SHA256SUMS"

  step "Republishing"
  gh release upload "$tag" "$work/SHA256SUMS" "$work/latest" \
     --repo "$github_repo" --clobber
  echo "  GitHub updated"

  if [ -n "${GITLAB_TOKEN:-}" ]; then
    api="https://gitlab.com/api/v4/projects/$gitlab_project"
    for f in SHA256SUMS latest; do
      curl --fail --silent --show-error \
           --header "PRIVATE-TOKEN: $GITLAB_TOKEN" \
           --upload-file "$work/$f" \
           "$api/packages/generic/alterion-open-project/$version/$f" >/dev/null
      echo "  GitLab updated: $f"
    done
    publish_descriptors_to_fixed_slot "$work"
  else
    warn "GITLAB_TOKEN not set, GitLab still has the old checksums"
  fi
fi

step "Done"
echo "Artefacts: $dist"
