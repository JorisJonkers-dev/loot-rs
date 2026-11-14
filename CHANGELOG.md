# Changelog

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
