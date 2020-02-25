[![Build Status](https://travis-ci.org/crates-io/criner.svg?branch=master)](https://travis-ci.org/crates-io/criner)

## Tasks

* [ ] Add 'InProgress' state to TaskState to prevent multiple processing runs to schedule the same task multiple times.
  * This happens, if downloads are big and slow or processing runs are done too frequently
  * Do this by adding a startup time to the state to track when the application was started. Then when starting to download something, set the task to inprogress.
    This has the problem that quitting while it is in progress will (probably not) call constructors in all cases (but validate that!). If constructors are called
    reliably, undo changes to the task on drop. If they are not reliable, have the task scheduler validate task states and if it encounters one that is in progress,
    it compares the startup time with the time the task was written. If the time is before the startup time, it's a left-over that needs to be reset to 'not started'
    and handled accordingly.
* [ ] resilience: protect against ThreadPanics - they prevent the program from shutting down
   * Futures has a wrapper to catch panics, even though we don't use it yet. A panic only brings down the future that panics, not the entire program.

## Lessons learned

* futures::ThreadPools - panicking futures crash only one thread
* long-running futures need error and potentially panick recovery. Futures has a panick catcher that could be useful.
