#!/usr/bin/env bash
# Push one SDK folder to its public mirror repo and tag the release there.
# Usage: tools/mirror-sdk.sh <prefix> <mirror-repo-url> <version>
# Example: tools/mirror-sdk.sh sdks/client/swift https://github.com/Torus403/notification-sender-swift.git 1.2.0
set -euo pipefail

prefix="$1"
mirror="$2"
version="$3"

split_sha="$(git subtree split --prefix "$prefix" HEAD)"
git push "$mirror" "$split_sha:refs/heads/main"
git push "$mirror" "$split_sha:refs/tags/$version"
