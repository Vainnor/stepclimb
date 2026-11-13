#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
# Fails when a source file lacks its SPDX license header in its first three lines. The license is
# AGPL-3.0-or-later everywhere except the SDK packages the plan licenses as Apache-2.0
# (packages/api-client). Covers tracked files and new files git does not ignore.
set -euo pipefail

apache='^packages/api-client/'
sources='\.(rs|ts|tsx|mjs|js|css|sql|sh|py)$|(^|/)Dockerfile$|^deploy/compose/.*\.ya?ml$|^\.githooks/'

missing=0
checked=0
while IFS= read -r file; do
    [[ -f "$file" ]] || continue
    if [[ "$file" =~ $apache ]]; then want="Apache-2.0"; else want="AGPL-3.0-or-later"; fi
    checked=$((checked + 1))
    if ! head -n 3 "$file" | grep -q "SPDX-License-Identifier: $want\b"; then
        echo "spdx: $file has no 'SPDX-License-Identifier: $want' in its first three lines" >&2
        missing=$((missing + 1))
    fi
done < <(git ls-files --cached --others --exclude-standard | grep -E "$sources" | sort -u)

if [[ $missing -ne 0 ]]; then
    echo "spdx: FAILED ($missing of $checked files)" >&2
    exit 1
fi
echo "spdx: ok ($checked files)"
