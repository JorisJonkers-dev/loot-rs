# Changelog

## [0.5.0](https://github.com/JorisJonkers-dev/loot-rs/compare/v0.4.0...v0.5.0) (2026-11-15)


### Features

* **api:** cancel and report progress of a sort, stable error codes, evaluate a condition, skip switches and a serde feature ([#52](https://github.com/JorisJonkers-dev/loot-rs/issues/52)) ([a1dda6d](https://github.com/JorisJonkers-dev/loot-rs/commit/a1dda6db448d23f125558ad3445656a721ffc0dc))
* **api:** choose the message language, state the Markdown contract and return plugin URLs ([#57](https://github.com/JorisJonkers-dev/loot-rs/issues/57)) ([efd0059](https://github.com/JorisJonkers-dev/loot-rs/commit/efd005903762b51354de609523bd20f559e38565))
* **api:** explain each move of the sorted order from the edges the sorter added ([#53](https://github.com/JorisJonkers-dev/loot-rs/issues/53)) ([2bdb61c](https://github.com/JorisJonkers-dev/loot-rs/commit/2bdb61c19dbc3efbb2f746c2d54cc1ce09ab41c7))
* **api:** identify the masterlist by its blob id and source, and replace its prelude as libloot does ([#54](https://github.com/JorisJonkers-dev/loot-rs/issues/54)) ([ec38612](https://github.com/JorisJonkers-dev/loot-rs/commit/ec38612ada1c4c2d0290be3c1dbd995b3f33981f))
* **api:** read a userlist and merge user metadata and user rules into the sort ([#55](https://github.com/JorisJonkers-dev/loot-rs/issues/55)) ([0d85b6d](https://github.com/JorisJonkers-dev/loot-rs/commit/0d85b6d4422334b253630f7ddd5a0d0c1545088b))
* **condition:** implement filename_version, description_contains and is_executable ([#59](https://github.com/JorisJonkers-dev/loot-rs/issues/59)) ([5aed745](https://github.com/JorisJonkers-dev/loot-rs/commit/5aed745a76147d4e77207cdc9096957d65f44b73))


### Bug Fixes

* **api:** report the progress of a stage in order when items finish on several threads ([#58](https://github.com/JorisJonkers-dev/loot-rs/issues/58)) ([6eab606](https://github.com/JorisJonkers-dev/loot-rs/commit/6eab60693a6b67b6d634c589b2578f8daf8d7cee))
* **metadata:** match plugin-name expressions as libloot does and check them on a table of libloot's answers ([#61](https://github.com/JorisJonkers-dev/loot-rs/issues/61)) ([ff0becf](https://github.com/JorisJonkers-dev/loot-rs/commit/ff0becf96f63fe18645286ef25452eaf611b149c))


### Performance Improvements

* **sort:** read plugins in parallel, compare overlaps ahead on threads and add an optional SortCache ([#51](https://github.com/JorisJonkers-dev/loot-rs/issues/51)) ([c7c42f2](https://github.com/JorisJonkers-dev/loot-rs/commit/c7c42f23c235065954849a4b9098d5a267e3c0bf))

## [0.4.0](https://github.com/JorisJonkers-dev/loot-rs/compare/v0.3.0...v0.4.0) (2026-11-14)


### Features

* **sort:** minimal-change sort mode that keeps the given order and moves only what a hard constraint forces ([#48](https://github.com/JorisJonkers-dev/loot-rs/issues/48)) ([5ed0584](https://github.com/JorisJonkers-dev/loot-rs/commit/5ed0584536c233f8251d4dab7e19e7325035c136))

## [0.3.0](https://github.com/JorisJonkers-dev/loot-rs/compare/v0.2.0...v0.3.0) (2026-11-14)


### Features

* **api:** take the game directory and additional data directories ([#41](https://github.com/JorisJonkers-dev/loot-rs/issues/41)) ([236849f](https://github.com/JorisJonkers-dev/loot-rs/commit/236849f4ba88c87309362b4ee1f2fe48bd959aa3))
* **cli:** name each JSON contract and add a contracts --json handshake ([#43](https://github.com/JorisJonkers-dev/loot-rs/issues/43)) ([3650cbd](https://github.com/JorisJonkers-dev/loot-rs/commit/3650cbd153bc70dfb2592b49742e65171fed08c8))
* **condition:** evaluate version() on plugins from the description field ([#45](https://github.com/JorisJonkers-dev/loot-rs/issues/45)) ([a2d710a](https://github.com/JorisJonkers-dev/loot-rs/commit/a2d710ad4b85737572fdc25016ffdb18506d0576))

## [0.2.0](https://github.com/JorisJonkers-dev/loot-rs/compare/v0.1.0...v0.2.0) (2026-11-14)


### Features

* **api:** emit schema_version in JSON and add helpers for a mod manager ([#24](https://github.com/JorisJonkers-dev/loot-rs/issues/24)) ([069b40a](https://github.com/JorisJonkers-dev/loot-rs/commit/069b40a3965918e025b79a81c28d49b58e970f21))
* **oracle:** trigger the private real-data oracle after a merge and post digest/real-data ([#22](https://github.com/JorisJonkers-dev/loot-rs/issues/22)) ([2e9ac74](https://github.com/JorisJonkers-dev/loot-rs/commit/2e9ac740b144445bfe23ebcc286235396a98adac))

## 0.1.0 (2026-11-14)


### Features

* add a library API that sorts a load order and returns messages and cleaning data ([#11](https://github.com/JorisJonkers-dev/loot-rs/issues/11)) ([f62108e](https://github.com/JorisJonkers-dev/loot-rs/commit/f62108e09a3db62d9f83163f6a63b4a841bbac72))
* add oracle_run, a local-only comparison with libloot on the real load order ([#20](https://github.com/JorisJonkers-dev/loot-rs/issues/20)) ([e008438](https://github.com/JorisJonkers-dev/loot-rs/commit/e008438bcaef65e07ccc79edd1d43538730ec662))
* read-only evaluator of LOOT masterlist rules for New Vegas ([#2](https://github.com/JorisJonkers-dev/loot-rs/issues/2)) ([e8ad685](https://github.com/JorisJonkers-dev/loot-rs/commit/e8ad685558a7ac386405d1290e2d49313a086bc6))
* **sort:** port libloot's plugin graph as the sorting algorithm ([#15](https://github.com/JorisJonkers-dev/loot-rs/issues/15)) ([9d20a4a](https://github.com/JorisJonkers-dev/loot-rs/commit/9d20a4afbb31199d1afc098aadfcf69e4f228b70))
* **sort:** return the messages LOOT generates when it checks an install ([#18](https://github.com/JorisJonkers-dev/loot-rs/issues/18)) ([e9a7921](https://github.com/JorisJonkers-dev/loot-rs/commit/e9a79214d9a576e84ca73ad7e6ac0dde37f5c12a))
* **sort:** sort by the archive files plugins have in common ([#17](https://github.com/JorisJonkers-dev/loot-rs/issues/17)) ([571d093](https://github.com/JorisJonkers-dev/loot-rs/commit/571d093cf358a0826b8591d9f721d5907aed8d53))
* **sort:** sort by the records plugins have in common ([#16](https://github.com/JorisJonkers-dev/loot-rs/issues/16)) ([e730fe8](https://github.com/JorisJonkers-dev/loot-rs/commit/e730fe895228608bad4478289b3c19fecbd9310c))
* version() and product_version() on executables behind an opt-in feature ([#8](https://github.com/JorisJonkers-dev/loot-rs/issues/8)) ([49608d9](https://github.com/JorisJonkers-dev/loot-rs/commit/49608d929995a6bf838f101842a371af6ea717f1))


### Bug Fixes

* read executables once and give the interpreter only a validated version block ([#13](https://github.com/JorisJonkers-dev/loot-rs/issues/13)) ([e3173cf](https://github.com/JorisJonkers-dev/loot-rs/commit/e3173cf8f714b5a6fd2f2117fa1f017291af3987))
* **release:** start versioning at 0.1.0, not 0.2.0 ([#14](https://github.com/JorisJonkers-dev/loot-rs/issues/14)) ([b475870](https://github.com/JorisJonkers-dev/loot-rs/commit/b4758702d2295f2a1b0590ce21a77975e8c28efa))
