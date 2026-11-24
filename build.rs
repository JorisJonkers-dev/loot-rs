// SPDX-License-Identifier: GPL-3.0-only
// Writes the ids module of the feature catalogue (catalogue/) for the tests, with the generator copy that the
// pinned tally version ships (`tally ids --generator catalogue/tally_ids_gen.rs`; `tally check` fails when it is
// stale). Standard library only: no build dependency.
include!("catalogue/tally_ids_gen.rs");

fn main() {
    tally_ids_gen::build("catalogue");
}
