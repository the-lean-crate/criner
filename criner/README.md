[![Build Status](https://travis-ci.org/crates-io/criner.svg?branch=master)](https://travis-ci.org/crates-io/criner)

## TODO

* [ ] Prettify html output to allow better diffing/better compression in git
* [ ] Be sure we handle YANKED crates correctly by skipping them when reporting at least and when downloading.
* [ ] Detect presence of git in report and auto-commit & push with progress, with ultra-high performance please!
* [ ] Make use of crates-io database easy and integrate with criner to allow using download counts and other meta-data
* [ ] Only show potential savings if it is the most recent version
* [ ] More reporting - right now the context gathering to see how much time is spent where is neglected.

## Possible Tasks
* [ ] Count negation patterns in includes and excludes. The latter don't seem to be working, and if nobody is using them, Cargo can either make it work or
      reject them properly. Maybe. Maybe first create an issue for that and see what they think.
* [ ] resilience: protect against ThreadPanics - they prevent the program from shutting down
   * Futures has a wrapper to catch panics, even though we don't use it yet. A panic only brings down the future that panics, not the entire program.
* [ ] Graceful shutdown on Ctrl+C
  * The current implementation relies on the database to handle aborted writes, and is no problem for that reason. However, it would be nice to have
    A well-behaving program.
    

## Lessons learned

* futures::ThreadPools - panicking futures crash only one thread
* long-running futures need error and potentially panic recovery. Futures has a panick catcher that could be useful.
* async_std channel blocks if there is no receiver, which can definitely bite you if your processors are down. Also I don't know why this is desirable behaviour.
* sqlite needs a lot of massaging to work acceptably in concurrent applications. Takeaway: WAL_mode, and when writting, always use immediate transactions
  when writing. Retry yourself while waiting and set a busy handler which waits.
* Trying to optimize output HTML for git by prettifying failed - I just couldn't see it improve anything. For debugging HTML, it's easiest to use the browser.

### When migrating to Sqlite

* sqlite…
  * is really not suited for many concurrent writers - you have to prepare for database locked errors, and the busy_handler doesn't help most of the time.
  * writing many small objects is slow, and can only be alleviated with prepared statements which are not always feasible or nice to use with a persistence
    design inspired by sled. To alleviate, the whole application must embrace Sqlite and work with statements directly.
  * Working with the lifetimes associated with transactions is a necessary evil, but it is painful too when trying to refactor anything! I just don't understand
    anymore what it tries to do, and have the feeling the compiler is confused itself (as in theory, there is no issue).
* sled databases are about 4 times bigger than an Sqlite database with the same content, and it would read about 1.2GB of a 14GB database at startup.
* sled is easy to handle in a threaded/concurrent environment, but iteration isn't possible across awaits as it's not sync
  * Sqlite is not sync nor is it send, so it needs more treatment before it can be used with spawened futures
* Zero-copy is straigforward with Sled as it provides IVec structs, which are handles into an LRU which is the backing store.
  * In retrospect, I would consider zero-copy a nice experiment, but also a premature optimization. It costs additinoal effort
    and when done from the beginning, you don't even know how much time is actually saved through that.
