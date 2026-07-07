# Changelog

All notable changes to this project are documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.0.3] - 2026-07-07

### Changed
- Updates no longer reset the operator's network mode: `network.env` is no longer tracked by git, so `git reset --hard` during an update leaves it untouched.

### Fixed
- Chart: drag-panning no longer gets stuck after a few pixels.
- Settings → Update app now shows the correct update commands (`git reset --hard` + image pull); the previous `git pull` fails on force-pushed releases.

## [0.0.2] - 2026-07-07

### Added
- Prebuilt Docker images and image-based Compose install (multi-arch).
- Documentation site with in-app update check.

### Changed
- Default network bind to localhost.

### Fixed
- findb: treat empty `FINDB_ARCHIVE_URL` as unset.

## [0.0.1] - 2026-07-06

[Unreleased]: https://github.com/G-OTW/OpenTraderWorld/compare/v0.0.3...HEAD
[0.0.3]: https://github.com/G-OTW/OpenTraderWorld/compare/v0.0.2...v0.0.3
[0.0.2]: https://github.com/G-OTW/OpenTraderWorld/compare/v0.0.1...v0.0.2
[0.0.1]: https://github.com/G-OTW/OpenTraderWorld/releases/tag/v0.0.1
