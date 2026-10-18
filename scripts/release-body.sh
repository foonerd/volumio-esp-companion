#!/bin/bash
# The release body for one version: the install steps, the version's
# changelog section, the sponsorship footer. The release workflow prints
# it into the release; the same command repairs a published one
# (gh release edit vX.Y.Z --notes-file <(scripts/release-body.sh X.Y.Z)).
set -euo pipefail
ROOT=$(CDPATH='' cd -- "$(dirname "$0")/.." && pwd)
VERSION=${1:?version, such as 0.1.0}
sed "s/{{VERSION}}/$VERSION/g" "$ROOT/.github/release-install.md"
printf '\n'
"$ROOT/scripts/release-notes.sh" "$VERSION"
printf '\n'
cat "$ROOT/.github/release-footer.md"
