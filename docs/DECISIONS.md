# Decisions

## `version()` on plugins: port, not a dependency

`version()` on a plugin needs two things: the version libloot takes from the description field (`extract_version`)
and the comparison of versions. The comparison lives in `loot-condition-interpreter` (MIT) as a type that is private
to that crate, and the crate pulls in `esplugin` (GPL-3.0, compatible with this crate's GPL-3.0). Both are small, so
both are ported to `src/version.rs`, with the licence notices in `NOTICE.md`. The default build therefore gains no
dependency, `deny.toml` is unchanged, and the plugin path is bounded by the header reader that already exists.
`loot-condition-interpreter` stays an optional dependency, for executables and DLLs only (feature
`executable-versions`).

Update: `executable-versions` is now a default feature. The New Vegas masterlist guards messages with
`version("../nvse_1_4.dll", >=, "0.6.0.0")` and the like; a build without the feature left those items out as
unsupported (issue #88). The interpreter's licence (MIT) and `esplugin`'s (GPL-3.0) were already allowed by
`deny.toml`. A caller that wants the smaller tree uses `default-features = false`.
