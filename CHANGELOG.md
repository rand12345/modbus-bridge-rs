# Changelog

## [0.3.3](https://github.com/rand12345/modbus-bridge-rs/compare/modbus-bridge-v0.3.2...modbus-bridge-v0.3.3) (2026-06-21)


### Bug Fixes

* **release:** use PAT for release-please tagging ([6d79fe5](https://github.com/rand12345/modbus-bridge-rs/commit/6d79fe54b6484a00336446a27edeb2fb6910d0a6))

## [0.3.2](https://github.com/rand12345/modbus-bridge-rs/compare/modbus-bridge-v0.3.1...modbus-bridge-v0.3.2) (2026-06-21)


### Bug Fixes

* **release:** keep dependency docs in sync ([d87540c](https://github.com/rand12345/modbus-bridge-rs/commit/d87540cee276b0effed6a2d193d2d83865615a05))
* **rtu:** correct feature-gated logging imports ([3312750](https://github.com/rand12345/modbus-bridge-rs/commit/3312750f9007692214b3aeca8b93ad49ea0d8374))
* **rtu:** retry transient async read errors ([bb4edda](https://github.com/rand12345/modbus-bridge-rs/commit/bb4edda05e6cf61d44239b71344aa909fc705d26))
* **rtu:** retry transient sync read errors ([a98a755](https://github.com/rand12345/modbus-bridge-rs/commit/a98a755e699375aefd5fdae0bf6ebca431a726ae))

## [0.3.1](https://github.com/rand12345/modbus-bridge-rs/compare/modbus-bridge-v0.3.0...modbus-bridge-v0.3.1) (2026-06-13)


### Bug Fixes

* **ci:** align Publish tag pattern with release-please component-prefixed tags ([a7866d0](https://github.com/rand12345/modbus-bridge-rs/commit/a7866d0f2f8b1a01a10a9e46e769b0a77f87f87d))

## [0.3.0](https://github.com/rand12345/modbus-bridge-rs/compare/modbus-bridge-v0.2.1...modbus-bridge-v0.3.0) (2026-06-13)


### Features

* add D=NoDelay generic to Bridge, BridgeBuilder, Connection; add async timeout impls ([81bbff3](https://github.com/rand12345/modbus-bridge-rs/commit/81bbff39d61684a57676b70e2b92abf429da590f))
* add embedded-hal-async and futures-util deps for timeout support ([45666bf](https://github.com/rand12345/modbus-bridge-rs/commit/45666bfcefe70796b992ce853e980666d98cd9ce))
* add NoDelay type and stub Client/ClientBuilder/ClientSession modules ([2030cd9](https://github.com/rand12345/modbus-bridge-rs/commit/2030cd902727dad99686da793c055448057c07a5))
* **event:** add BridgeError::RtuClosed and BridgeError::Timeout ([b064a97](https://github.com/rand12345/modbus-bridge-rs/commit/b064a97bad1ecfecdef94b2ed54382f8be6b1efe))
* **fuzz:** fix crate name, expose frame funcs via __fuzzing, add fuzz_client_session ([3ade95e](https://github.com/rand12345/modbus-bridge-rs/commit/3ade95ec3e870eb466dd2523770a5f61584c0311))
* implement Client and ClientBuilder (RTU-&gt;TCP direction) ([3957fbb](https://github.com/rand12345/modbus-bridge-rs/commit/3957fbb01ce354c8bcda27439cee2fd5ac97de57))


### Bug Fixes

* **ci:** bump embedded-io-adapters to 0.7 to match embedded-io-async ([3c9fc8a](https://github.com/rand12345/modbus-bridge-rs/commit/3c9fc8a0ce227c05ca49fc15920636f1850fdfd6))
* **ci:** fmt and clippy::should_implement_trait on sync next() methods ([0f7dff2](https://github.com/rand12345/modbus-bridge-rs/commit/0f7dff21c5e7531d8c37da14e1b2c2e6379ff06a))
* **ci:** use --no-default-features for all sync jobs ([e431b27](https://github.com/rand12345/modbus-bridge-rs/commit/e431b27afce658f522709eb862538d4313af2209))
* TID-mismatch fallback in ClientSession must use rx_tid not literal 0 ([c18dbda](https://github.com/rand12345/modbus-bridge-rs/commit/c18dbda6ef2bbedb6d71eee7127fc5da9886fb9a))
