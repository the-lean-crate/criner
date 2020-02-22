
## Tasks

* [x] Move data types to model module
* [x] replace from traits with macro
* [x] tree-access can be generalized - do it for each type we store
* [x] integrate 'context' tree into base trait as much as feasible
* [x] replace async-io with futures-rs for future-proofing
* [x] integrate async progress
* [x] downloads with backpressure
* [x] _(investigate)_ resumable downloads
   *  no need to do that now as it complicates things. But it's totally possible. There is a reqwest crate that might do that already.
* [ ] resilience: protect against ThreadPanics - they prevent the program from shutting down
   * Futures has a wrapper to catch panics, even though we don't use it yet. A panic only brings down the future that panics, not the entire program.

## Lessons learned

* futures::ThreadPools - panicking futures crash only one thread
* long-running futures need error and potentially panick recovery. Futures has a panick catcher that could be useful.
