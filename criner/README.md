[![Build Status](https://travis-ci.org/crates-io/criner.svg?branch=master)](https://travis-ci.org/crates-io/criner)

## Tasks

* [ ] review and eliminate some of the 'silly' copies we now make after removing zero-copy
* [ ] Make all tree methods take an actual key and get rid of the insertion item - it's the reason for being so unwieldy
* [ ] Store Task by name and version - otherwise they can never change in case they do change.
      Can be done quickly without redownloading by checking for presence of file or with migration (better)
* [ ] remove io/cpubound duplication and generalize tasks
  * [ ] allow task dependencies, so that one triggers another potentially. Fixes that right now, two processing runs are needed
        to untar downloads
* [x] export all data into a flattened queryable sqlite database
* [x] also write data to sqlite when fetching
  * [ ] experiment with SQLITE Pragmas: performance(journal_mode, journal_size, synchronous=0), read_uncommitted
* [ ] resilience: protect against ThreadPanics - they prevent the program from shutting down
   * Futures has a wrapper to catch panics, even though we don't use it yet. A panic only brings down the future that panics, not the entire program.

## Lessons learned

* futures::ThreadPools - panicking futures crash only one thread
* long-running futures need error and potentially panick recovery. Futures has a panick catcher that could be useful.

### When migrating to Sqlite

* sled databases are about 4 times bigger than an Sqlite database with the same content
* sled is easy to handle in a threaded/concurrent environment, but iteration isn't possible across awaits as it's not sync
  * Sqlite is not sync nor is it send, so it needs more treatment before it can be used with spawened futures
* Zero-copy is straigforward with Sled as it provides IVec structs, which are handles into an LRU which is the backing store.
  * In retrospect, I would consider zero-copy a nice experiment, but also a premature optimization. It costs additinoal effort
    and when done from the beginning, you don't even know how much time is actually saved through that.
