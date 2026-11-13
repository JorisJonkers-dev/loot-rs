# loot-fnv

Read-only evaluation of [LOOT](https://github.com/loot/loot) masterlist rules (requirements, load-after, tags)
over a Fallout: New Vegas / Tale of Two Wastelands load order, in Rust, for use on Linux.

**Licence: GPL-3.0**, the licence of LOOT. This is a port in the licensing sense: it follows LOOT's rule semantics
and is kept under LOOT's licence so that nothing here can conflict with it. LOOT's authors and project are at
https://github.com/loot/loot; the masterlist is community data under its own licence and is read at run time,
never copied into this repository.

Status: just started. Scope and design to follow in `docs/`. The plugin header reading and the master-based
sort that it will build on live in `tools/fnvorder` of the NV-Linux-Modding repository (MIT OR Apache-2.0,
written from the file format, no LOOT code). Nothing from this repository flows back there.

Verify the licence of the exact LOOT version used before copying any behaviour: the notes this plan came from
quote it from memory.
