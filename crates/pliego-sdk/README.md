# pliego-sdk

`pliego-sdk` is the protocol authority for PliegoRS OpenSDK preview contracts.
It validates extension identity, host compatibility, declared powers, byte
identity, resource budgets, WIT packages, and effect receipts before an
extension can execute.

The crate does not grant ambient filesystem, network, environment, clock, or
random access. A host must create an explicit `CapabilityPolicy`; brokered
effect attempts produce ordered, digest-bound success or error receipts.

Normative WIT contracts ship in this crate under [`wit`](wit). The public
[JSON schemas](https://github.com/celiumsai/pliegors/tree/main/schemas) and
[OpenSDK RFC](https://github.com/celiumsai/pliegors/blob/main/docs/rfc/RFC-006-opensdk-planes-and-capabilities.md)
live in the PliegoRS repository.
