[![Build Status](https://travis-ci.org/crates-io/criner.svg?branch=master)](https://travis-ci.org/crates-io/criner)

## Possible Tasks

* [ ] Detect presence of git in report and auto-commit & push with progress
* [ ] Make use of crates-io database easy and integrate with criner to allow using download counts and other meta-data
* [ ] Only show potential savings if it is the most recent version
* [ ] Count negation patterns in includes and excludes. The latter don't seem to be working, and if nobody is using them, Cargo can either make it work or
      reject them properly. Maybe. Maybe first create an issue for that and see what they think.
* [ ] resilience: protect against ThreadPanics - they prevent the program from shutting down
   * Futures has a wrapper to catch panics, even though we don't use it yet. A panic only brings down the future that panics, not the entire program.
* [ ] Graceful shutdown on Ctrl+C
  * The current implementation relies on the database to handle aborted writes, and is no problem for that reason. However, it would be nice to have
    A well-behaving program.
    
## FAQ

### It keeps claiming that my included files are waste !?

It detecs files included via `include_str!(…)` and `include_bytes!(…)`, but only so in in `lib.rs` and `main.rs`, or other binary targets.

### How can I just make it stop complaining ?

Add the `include = […]` that it proposes, possibly altered to your liking and needs. It will still provide you with potential negated include
patterns to exclude tests, docs.

### What's better, exclude directives or include directives?

The waste report favors include directives, as it will not mark any file as wasted if present, but make recommendations on how to save even more
by excluding tests, docs and the likes.

When excludes are present, it makes recommendations mandatory, and considers all files that don't are included despites those recommendations to
be waste. The reason is that whitelists, i.e. include directives, are better supported by cargo due to the presence of negations, so it assumes
people have better control over the includes they make.
    
## Limitations of Waste Reporting

* only extracts strings and 'rerun-if' directives from build.rs files.
* It does not know renamed `build.rs`, `lib.rs` or `main.rs` files.
  * It could learn about renamed files by changing it's algorithm when gathering crate information.
* it does not handle negated include patterns, but it also is not disturbed by them
* When replacing an exclude which is not specific enough with an include that is, it always resolves to all desirable files and is unable 
  to generate a glob pattern from that. This can result in many files suggested as include.


## Fun facts

* On a venerable 5 year old quad-core MBP…
   * …it takes about 10min to extract all 216k crate versions (~42GB on disk) in memory. This time is needed to extract all file meta-data and
     interesting files like Cargo.toml/Cargo.lock, lib.rs and bin.rs and store that data in sqlite. Decompressed that would be 145GB worth of data,
     so it processes about 240MB per second. My feeling is that it bottlenecks on writing the result to SQLite.
   * it takes about 4min to generate a report for all 216k crate versions, including aggregations for each crate and for crates.io, and write
     them to disk. 
* Both of the above only happen once, as from that point on all else is incremental, reducing the amount of unnecessary work to close to zero.

## Lessons learned

* futures::ThreadPools - panicking futures crash only one thread
* long-running futures need error and potentially panic recovery. Futures has a panick catcher that could be useful.
* async_std channel blocks if there is no receiver, which can definitely bite you if your processors are down. Also I don't know why this is desirable behaviour.
* sqlite needs a lot of massaging to work acceptably in concurrent applications. Takeaway: WAL_mode, and when writting, always use immediate transactions
  when writing. Retry yourself while waiting and set a busy handler which waits.

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
