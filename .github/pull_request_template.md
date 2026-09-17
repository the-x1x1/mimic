## What

## Why

## Evidence

- [ ] `.\scripts\test.ps1` green locally
- [ ] docs/PROJECT_STATUS.md updated if implementation status changed
- [ ] CHANGELOG.md entry under the unreleased/next version
- [ ] Schema change → new migration file + migration test
- [ ] Lightroom apply path change → fake-bridge test updated, NEEDS-REAL-LIGHTROOM-QA noted

## Safety checklist (CLAUDE.md)

- [ ] No `.lrcat` access, no source-media writes, no XMP mutation
- [ ] Unknown Lightroom settings still preserved
- [ ] No claim of a working apply without read-back verification
