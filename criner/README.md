[![Build Status](https://travis-ci.org/crates-io/criner.svg?branch=master)](https://travis-ci.org/crates-io/criner)

## Tasks

* [ ] export all data into a flattened queryable sqlite database
* [ ] also write data to sqlite when fetching
  * [ ] experiment with SQLITE Pragmas: performance(journal_mode, journal_size, synchronous=0), read_uncommitted
* [ ] resilience: protect against ThreadPanics - they prevent the program from shutting down
   * Futures has a wrapper to catch panics, even though we don't use it yet. A panic only brings down the future that panics, not the entire program.

## Lessons learned

* futures::ThreadPools - panicking futures crash only one thread
* long-running futures need error and potentially panick recovery. Futures has a panick catcher that could be useful.
