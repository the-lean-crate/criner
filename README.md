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

**How it works**

_Criner_ currently operates in three stages when executed with `criner mine`:

* **input**
  * **new versions crates-io repository**
    * Use the crates.io git index to learn about new crates incrementally
  * _[PLANNED]_ **Download the crates.io SQL dump** for more meta-data and download counts
* **processing**
  * **traverse all crate versions** and **schedule** tasks or re-schedule failed tasks. Tasks will spawn other tasks if task processors are free,
    to keep all processors busy. A **processor** is a light-weight future which receives tasks by a channnel. Once a task is done, it will not
    be processed again, allowing for incremental processing.
  * **task types**
    * **download** - downloads the crate archive and stores it on disk. This will need 39GB as of 2020-03-18. 
    * **extraction** - extract the crate in memory and store all paths metadata, and some interesting files like `Cargo.toml` in full up to 128kb in size.
      As of 2018-03-18 it takes 10min to process all 215k crate versions on a 5year old MBPro with 4 physical cores.
    * _[PLANNED]_ **Sloc** - count using tokei.
    * _[PLANNED]_ **Geiger** - count (amount of unsafe code) using `cargo geiger`.
* **reporting**
  * Traverse all crate versions and write a report file for each one. Aggregate all versions of a crate and write a report for each crate. Aggregate all
    crates and write a report for all crates on crates.io and all their versions. This works incrementally by leveraging the fact that crate versions are
    immutable, and that only new ones are added.
  * **report types**
    * **Waste** - aggregate the amount additional files which are not needed to build the package.
    * _[PLANNED]_ **Geiger** - Show the amount of unsafe code in a crate version and possibly its dependencies.
    * _[POSSIBLE]_ **Speed** - Using the sloc count of the crate and its dependencies, how much build time will be added to your project by using it 
     (in the worst case). The MVP might just be the SLOC count of a crate version and it's dependencies, similar to what lib.rs offers.
* **sharing**
  * _[PLANNED]_ **Auto-commit & push reports** - That way as reports are updated, they are pushed to github with minimial delay and while providing progress to the user.
  
**Running Criner at home**

Clone this repository and run `cargo run --release -- mine` to get started. Provided criner is allowed to finish, it will require about 46GB of disk space as of 2020-03-18.
  
**Criner for data science**

Provided there is a database generated already with `criner mine`, run `criner export` to get another SQlite database with all data exploded into tables and fields, which
can be operated using SQL. This process is non-incremental and takes about 5 minutes to complete on a single core. Threading is not implemented.

Possible improvements are along export performance - it could probably be parallel and incremental - and along not having to mine yourself for an initial database state.
Criner could upload its database once a day to an S3 bucket for instance - it's about 800MB gzipped.

# The Lean Crate Initiative

Is my attempt to improve build times by reducing download and extraction times. This makes the ecosystem more approachable to people or regions with slow internet and thus
is very relevant for inclusiveness and extending Rusts reach.

This is facilitated by three means:

* **The Criner Waste Report** - Analyse the current state of waste within all crate versions of crates.io and offer a **fix**.
* _[PLANNED]_ **The 'cargo-diet' companion program** - Start lean by default and compute optimial includes and exludes before publishing to crates.io.
* _[PLANNED]_ **The 'lean crate'** badge - Show off that you care and present the badge on crates.io and in README files.

# The Criner Waste Report

As the first part of _The Lean Crate Initiative_, this report provides the data needed to see if this is a problem worth solving in the first place.
And as of 2020-03-18, initial numbers show that out of 147GB of uncompressed crates data, 64GB or 44% are _most probably_ not required to build a crate.

The report operates on the following assumptions:

* crates.io is a distribution platform for Rust source code
* the source code is distributed for the purpose of compiling the crates and should be self-contained

From these assumptions, some conclusions can be drawn.
There is no need for…

* …benchmarks
* …tests
* …docs
* …fixtures
* …anything else that is used for development
* …packagers can use source archives from Github or clone repositories for the data they need to run tests

Based on these assumptions and conclusions, _The Criner Waste Report_ computes a suggestions for new `include` or `exclude` directives which prevent
unnecessary data to be put into the crate archive.

Due to the way Cargo handles these directives, `include` directives are deemed most powerful in the persuit of keeping the amount of patterns small, using
negative patterns where needed. Thus these will be recommended whenever possible. 

## FAQ

### How dare you call anything in my crate 'Waste' ??!

Apologies, the term was proposed by the marketing department who believed that 'The Criner Waste Report' will do better than 
'The Criner Report of files you do not need to build a crate'.

The author does not have any feelings towards crates that are bigger than they _probably_ have to be, and is happy to help get your crate
off the index. Some files listed are certainly false positivies due to [limitations], read on in this FAQ to learn how to remove these
false positives.

[limitations]: https://github.com/crates-io/criner#limitations-of-waste-reporting

### It claims my crate is full of waste because it doesn't see what the build-script requires ?!

Indeed the Waste Report does its best to extract names from build scripts, but won't be able to resolve things like `format!("C-lib-1.0.23-{}", suffix)`.
To resolve this, set your own `include` directive. The Criner Waste Report will help finding even better includes from that point on, but it will merely
be a suggestion, trusting that you set includes exactly the way they are needed.

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

* It Only extracts strings and 'rerun-if' directives from build.rs files. Thus it won't be able to deal with runtime generated strings or paths
  very well. Additionally it does have filter logic to reduce the input set of extracted strings which might yield false positives.
* It does not handle negated include patterns, but it also is not disturbed by them.
* When replacing an exclude which is not specific enough with an include that is, it always resolves to all desirable files and is unable 
  to generate a glob pattern from that. This can result in [many files suggested as include](https://crates-io.github.io/waste/gnir/0.9.7.html).

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
