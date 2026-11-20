# Local source candidate and dependency review (P5.1)

This packet prepares an auditable Git source snapshot, not a GPL compliance determination or permission to redistribute. Preserve original upstream and PR #4 attribution. Do not publish binaries or this source candidate before the outstanding review.

## Generate (Python 3.11+, Git)

From a clean committed checkout:

```sh
python3 scripts/linux/test-source-candidate.py
python3 scripts/linux/source-candidate.py /tmp/bushido-source-$(git rev-parse --short HEAD)
```

Choose a fresh output directory outside the checkout. No network, package install or app build is performed. The archive uses git archive at exact HEAD, then verifies every regular file against the full committed tree (Git blob hash and executable bit). Missing/extra/substituted members, symlinks and submodule entries fail explicitly. There is no extraction to the filesystem, no worktree or untracked content in the archive. Gzip timestamp/name are fixed; repeatability is a same-toolchain candidate property, not independently accepted reproducibility. A failed generation may leave a private incomplete directory; only a final receipt identifies a completed candidate.

Outputs: source-candidate.tar.gz, dependency-inventory.json, receipt.json. The receipt contains the full commit, lock hashes, archive/inventory hashes and verified tracked-file count. It always sets publication_authorized=false and corresponding_source_review=OPEN.

## Release review ledger

| Obligation | Evidence produced | Remaining |
| --- | --- | --- |
| Application and local forks | Full Git tree, manifest licenses and notice-file paths | Review source completeness, notices, modifications and upstream attribution |
| Rust registry packages | Name/version/source/checksum from Cargo.lock | Resolve actual target/features with locked cargo metadata, capture exact dependency source and license texts, review vendored build scripts and generated code |
| npm dependencies | Lock entries, resolved/integrity/declared license and dev/optional markers | Actual installed target graph, bundled output/module attribution, exact source and notices |
| Filter lists and scriptlet assets | Original tracked bytes and SHA-256; selected declared header lines | Review each original source, revision, license terms, generator and asset redistribution rights; do not infer license from URL/header alone |
| Native system libraries | Existing packaging/dependency contract | Resolve actual ELF dependencies and system-library/source obligations for target distro |
| Binary/source relationship | Candidate commit and lockfile hashes | Build same commit, compare build receipt, archive relevant generated inputs and identify exact binary digest; no publication yet |

A lockfile is a resolution record, not a copy of dependency source or proof of license compatibility. No external source vendoring, license adjudication, binary production, source offer, upload, signing, or hosted CI is included. Review corresponding-source requirements for the actual distribution method and artifact before release.
