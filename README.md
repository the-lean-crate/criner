[![Build Status](https://travis-ci.org/crates-io/criner.svg?branch=master)](https://travis-ci.org/crates-io/criner)

_Criner_ is a platform to make incrementally mining crates.io easy and affordable for everyone. _Criner_ is fast, configurable to use all
available bandwidth and CPU, while keeping the memory footprint low enough to comfortably run on small devices with less than 512MB of RAM.

**Motivation**

I live in China and learned to live with slow and flaky internet connections. Every byte that reaches my computer makes me cheer in joy,
and I don't get to cheer a lot recently 😅.

I made _Criner_ to help me reduce the average download size of crates, triggered by the realization that [`nushell`][nu] sent me 3MB of 
images in a 4MB download. The [fix](https://github.com/nushell/nushell/pull/1316) was trivial, and I wondered how much more there was to
gain by simple fixes. The idea for *The Criner Waste Report* was born.

[nu]: https://github.com/nushell/nushell

# The Criner Waste Report

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


# Operating Manual

## How to run migrations

As migrations are currently special purpose programs that may eat laundry for breakfast, they cannot be executed by accident.
```
RUST_LOG=info cargo run --features migration  --  migrate
```
