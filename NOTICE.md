# Notices

## `src/plugin.rs` (MIT)

Copied from `tools/fnvorder/src/plugin.rs` in
[JorisJonkers-dev/NV-Linux-Modding](https://github.com/JorisJonkers-dev/NV-Linux-Modding)
(fnvorder, offered under MIT OR Apache-2.0; the MIT option is taken). It was written from the
public plugin file-format description and contains no LOOT code. It is kept as a separate module.
No changes have been made to it in this repository.

```text
MIT License

Copyright (c) the fnvorder authors

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and
associated documentation files (the "Software"), to deal in the Software without restriction,
including without limitation the rights to use, copy, modify, merge, publish, distribute,
sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or
substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT
NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND
NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM,
DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT
OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
```

## `src/sort.rs`, `src/sort/` (GPL-3.0-or-later, ported from libloot)

The sorter is a port to Rust of the plugin graph of [libloot](https://github.com/loot/libloot) (version 0.29.7,
`src/sorting/`: `plugins.rs`, `groups.rs`, `search.rs`, `validate.rs`; GPL-3.0-or-later, Copyright (C) the libloot
authors), following its documentation of the algorithm
(<https://loot-api.readthedocs.io/en/latest/api/sorting.html>). The graph stages, the order in which edges are
added, the bidirectional search, the tie-break pinning and the group depth-first searches are libloot's; the
unit tests in `src/sort/plugins_tests.rs` are libloot's own tests of the same functions, translated. What differs:
vertices are indices instead of petgraph nodes (petgraph's iteration orders, on which libloot's results depend, are
reproduced in `src/sort/search.rs`); userlist (user metadata) edges and Starfield's blueprint masters are not
ported; a group that nobody defined is left without edges instead of being an error. Every ported file says so in
its header. libloot is licensed GPL-3.0-or-later and this crate GPL-3.0-only: the combination is distributed under
GPL-3.0, which the licence allows.

## `src/records.rs` (GPL-3.0-or-later)

The record reader is written for this crate (a flat scan of the record headers of a Fallout: New Vegas plugin, with
every size checked against the file). What it computes is libloot's and esplugin's: a record id is the FormID resolved
against the plugin's masters (master name and object index; a master byte past the master list means the plugin
itself), a plugin overrides a record when its master byte names a master, and two plugins overlap when they share a
resolved id. That follows `esplugin` (<https://github.com/Ortham/esplugin>, `src/plugin.rs` and `src/record_id.rs`,
GPL-3.0, Copyright (C) 2017 Oliver Hamlet) as libloot uses it (`Plugin::do_records_overlap` and
`override_record_count`). The file header says so.
