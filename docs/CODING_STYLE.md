# Peinit Coding Style

Follow the [PSC coding rules](https://learn.peios.org/psc/coding-style/)
and the Rust language profile. `tools/style-check.py` pins the mechanical
checks; `tools/check` also runs Clippy, tests, ABI checks, and packaging.
Use four spaces and a 100-column Rust formatter target. Import order is
preserved so formatting cannot detach module comments from their subjects.

Keep models and pure state transitions separate from supervisor orchestration
and OS effects. Preserve the existing C ABI, wire names, registry names,
package identities and boot executable name during style work. Some internal
Rust modules remain public for source compatibility despite being hidden from
API documentation; narrow private implementation visibility where compatible.

Explain unsafe preconditions and ownership at the point where they matter.
Prefer owned file descriptors and automatic cleanup. PID 1 fork paths have
additional restrictions: avoid introducing allocation, locks, or threads after
fork without reviewing the process boundary and its failure handling.

Long state-machine dispatches and table-driven tests can stay together when
splitting would hide the transition contract. Review long functions for
separable responsibilities; do not split them solely to satisfy a line count.
Generated ABI snapshots and vendored PPK sources follow their generators or
source project. Historical recipes under `docs/archive/` are reference records,
not active build or style inputs.
