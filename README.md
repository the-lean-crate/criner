
## Tasks

* [x] Move data types to model module
* [x] replace from traits with macro
* [x] tree-access can be generalized - do it for each type we store
* [x] integrate 'context' tree into base trait as much as feasible
* [x] replace async-io with futures-rs for future-proofing
* [x] integrate async progress
* [ ] downloads with backpressure
* [ ] _(investigate)_ resumable downloads
* [ ] resilience: protect against ThreadPanics - they prevent the program from shutting down

## Lessons learned

* futures::ThreadPools - panicking futures crash only one thread
* long-running futures need error and potentially panick recovery. Futures has a panick catcher that could be useful.
