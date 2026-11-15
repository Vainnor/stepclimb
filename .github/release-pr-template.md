## What changes

This PR promotes `next` to `main` as Stepclimb $version. `main` holds released code and `next`
is where every PR lands first; this release PR is the only way changes reach `main`. The version
bump and the notes below are already on `next`, merged through the release preparation PR.

Merge it with a **merge commit**, not a squash, and keep `next` (`docs/adr/0030-release-merge-commits.md`).

## Before and after

```
before                         after the merge
main  o                        main  o-------------M   <- tag $version
       \                              \           /
next    o--o--o--o                     o--o--o--o      next is unchanged
```

## Release notes

$notes

## Certification

- [x] I certify this contribution under the [Developer Certificate of Origin](https://developercertificate.org/) (`CONTRIBUTING.md`)

## Gates

<!-- `just ci` summary lines on next's HEAD and the OS; anything not run, and why. -->

## After the merge

1. `git fetch origin && git tag -a $version -m "Stepclimb $version" origin/main && git push origin $version`
2. `git worktree add ../stepclimb-$version $version`, then in it `just release $version`; it
   checks the tag, runs `just ci` and prints the `gh release create` command.
