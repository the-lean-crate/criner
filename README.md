[![Build Status](https://travis-ci.org/the-lean-crate/criner.svg?branch=master)](https://travis-ci.org/the-lean-crate/criner)

# The Lean Crate Initiative

We want to improve build times by reducing download and extraction times. This makes the ecosystem more approachable to people or regions with slow internet and thus
is very relevant for inclusiveness and for extending Rusts reach.

This is facilitated by three means:

* **[The Criner Waste Report][waste-io]** - Analyse the current state of waste within all crate versions of crates.io and offer a **fix**.
* _[PLANNED]_ **The 'cargo-diet' companion program** - Start lean by default and compute optimial includes and exludes before publishing to crates.io.
* _[PLANNED]_ **The 'lean crate' badge** - Show off that you care and present the badge on crates.io and in README files.

## Motivation

I live in China and learned to live with slow and flaky internet connections. Every byte that reaches my computer makes me shed a tear in joy.

This initiative was motivated by a [`nushell`][nu] update which took forever and failed multiple times when trying to send me 3MB of 
images in a 4MB download. The [fix][nu-fix] was trivial, and I wondered how much more there was to gain by simple fixes like that.
The idea for _The Criner Waste Report_ was born, which soon turned into a multi-step plan to tackle this problem.

Nowadays, `nushell` is [perfectly lean][nu-lean], and I hope we will have more of these crates as the initiative progresses.

[nu]: https://github.com/nushell/nushell
[nu-fix]: https://github.com/nushell/nushell/pull/1316
[nu-lean]: https://crates-io.github.io/waste/nu/0.11.0.html

## How you can help right now

First of all, thanks so much for your willingness to help! Let's get started.

Head over to The [Criner Waste Report][waste-io] and find your crate or jump to your crate directly using `https://crates-io.github.io/waste/<your-crate>`. 
See if a lot of 'waste' is detected, and validate and try the suggested fix. If something is wrong or not working, click the **Provide Feedback** link 
at the bottom of your crates page.

### Example: There is some 'Waste' to be removed

* Head over to [your most recent published crate version](https://crates-io.github.io/waste/rusty-leveldb/0.3.3.html)
* Create a new `include` directive, with values suggested by the page above, i.e. `include = ["src/**/*", "LICENSE", "README.md", "!**/benches/*"]`.
* See if it works for you. And if it does, publish a new version. This will adjust the crate ranking next time the Criner authors update the website, currently once a day.

### Example: The crate is lean - there is nothing to do, or is there?

* Head over to [your most recent published crate version](https://crates-io.github.io/waste/ripgrep/12.0.0.html) just to see the crate is perfectly lean!
* Maybe check if Criner might have missed something - a way to do this is to check the package it would upload
   * `cargo package --offline --allow-dirty --no-verify`
   * Find the package in `target/package` and untar+gz it using `tar -xzf target/package/<crate-version>.crate`.
   * Browse the extracted content and see if there is more than you think should be there.
* If you found files that are not required and you think _The Criner Waste Report_ should pick them up, use the **Provide Feedback** link at the bottom
  of the page and report the issue. 

# [The Criner Waste Report][waste-io]

As the first part of _The Lean Crate Initiative_, this report provides the data needed to see if this is a problem worth solving in the first place.
And as of 2020-03-18, initial numbers show that out of 147GB of uncompressed crates data, 59GB or 40% are _most probably_ not required to build a crate.

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
* _(…packagers can use source archives from Github or clone repositories for the data they need to run tests)_

Based on these assumptions and conclusions, _The Criner Waste Report_ computes a suggestions for new `include` or `exclude` directives which prevent
unnecessary data to be put into the crate archive.

Due to the way Cargo handles these directives, `include` directives are deemed most powerful in the persuit of keeping the amount of patterns small, using
[negative patterns][negative-include] where needed. Thus these will be recommended whenever feasible. 

This part of the initiative is [still under heavy development][criner-todo], but available as _ugly alpha_.

Please do note that _your feedback_ on whether or not these assumptions and conclusions are correct is much appreciated, everything can be changed
to make _The Criner Waste Report_ better in a [collaborative][code-of-conduct], community driven effort.

[criner-todo]: https://github.com/the-lean-crate/criner/tree/master/criner#todo
[waste-io]: https://crates-io.github.io/waste/
[negative-include]: https://doc.rust-lang.org/cargo/reference/manifest.html#the-exclude-and-include-fields
[code-of-conduct]: https://github.com/the-lean-crate/criner/blob/master/CODE_OF_CONDUCT.md

## FAQ

### How dare you call anything in my crate 'Waste' ??!

Apologies, the term was proposed by the marketing department who believed that 'The Criner Waste Report' will do better than 
'The Criner Report of files you do not need to build a crate'.

The author does shame crates that are bigger than they _probably_ have to be, and is happy to help get your crate
off the index. Some files listed are certainly false positivies due to [limitations], read on in this FAQ to learn how to remove these
false positives.

[limitations]: https://github.com/the-lean-crate/criner#limitations-of-waste-reporting

### It claims my crate is full of waste because it doesn't see what the build-script requires ?!

Indeed the Waste Report does its best to extract names from build scripts, but won't be able to resolve things like `format!("C-lib-1.0.23-{}", suffix)`.
To resolve this, set your own `include` directive. _The Criner Waste Report_ will help finding even better includes from that point on, but it will merely
be a suggestion, trusting that you set includes exactly the way they are needed.

### It keeps claiming that my included files are waste !?

It detecs files included via `include_str!(…)` and `include_bytes!(…)`, but only so in in `lib.rs` and `main.rs`, or other binary targets.

### How can I just make it stop complaining ?

Add the `include = […]` that it proposes, possibly altered to your liking and needs. It will still provide you with potential negated include
patterns to exclude, for instance, tests and docs.

### What's better, exclude directives or include directives?

The waste report favors include directives, as it will not mark any file as wasted if present, but make recommendations on how to save even more
by excluding tests, docs and the likes.

When excludes are present, it makes recommendations mandatory, and considers all files that don't are included despites those recommendations to
be waste. The reason is that whitelists, i.e. include directives, are better supported by cargo due to the presence of negations, so it assumes
people have better control over the includes they make.

### What are "potential savings"?

It's our way to hint at the possibility of making your crate smaller while acknowledging that your `include` directive is probably exactly what you
had in mind when designing it.

However, right now we believe that certain kinds of files are not needed to build a crate and thus may have additional negation patterns that would
exclude these files. Common examples are tests, which are easily included by the typical `src/**/*.rs` include directive.

Potential savings do not count as 'Waste', but currently prevent the [crate version](https://crates-io.github.io/waste/zfs-core/0.2.0.html) 
from achieving the `perfectly lean` status.
    
## Limitations of Waste Reporting

* It Only extracts strings and 'rerun-if' directives from build.rs files. Thus it won't be able to deal with runtime generated strings or paths
  very well. Additionally it does have filter logic to reduce the input set of extracted strings which might yield false positives.
* It does not handle negated include patterns, but it also is not disturbed by them.
* When replacing an exclude which is not specific enough with an include that is, it always resolves to all desirable files and is unable 
  to generate a glob pattern from that. This can result in [many files suggested as include](https://crates-io.github.io/waste/gnir/0.9.7.html).
* Cargo traverses the entire manifest directory and applies its globsset. The globs in that globsset are very greedy, so that what looks like a file
  `README.md` will actually match everything that matches `*/README.md`  or `**/README.md`. This may include unwanted files and we will not detect these.
  Circumvent this yourself by using a prefix slash, such as in `/README.md` indicating the file must be in the top level.

## Fun facts

* On a venerable 5 year old quad-core MBP…
   * …it takes about 10min to extract all 216k crate versions (~42GB on disk) in memory. This time is needed to extract all file meta-data and
     interesting files like Cargo.toml/Cargo.lock, lib.rs and bin.rs and store that data in sqlite. Decompressed that would be 145GB worth of data,
     so it processes about 240MB per second. My feeling is that it bottlenecks on writing the result to SQLite.
   * it takes about 3.5min to generate a report for all 217k crate versions, including aggregations for each crate and for crates.io, and write
     them to disk. 
* Both of the above only happen once, as from that point on all else is incremental, reducing the amount of unnecessary work to close to zero.


# Criner

_Criner_ is a platform to make incrementally mining crates.io easy and affordable for everyone. _Criner_ is fast, configurable to use all
available bandwidth and CPU, while keeping the memory footprint low enough to comfortably run on small devices with less than 512MB of RAM.

## How it works

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
  
## Running Criner at home

Clone this repository and run `cargo run --release -- mine` to get started. Provided criner is allowed to finish, it will require about 46GB of disk space as of 2020-03-18.
  
## Criner for data science

Provided there is a database generated already with `criner mine`, run `criner export` to get another SQlite database with all data exploded into tables and fields, which
can be operated using SQL. This process is non-incremental and takes about 5 minutes to complete on a single core. Threading is not implemented.

Some of the columns are of type `JSON`, whose properties can be used in queries using the `json_*(…)` set of SQLITE functions.

Possible improvements are along export performance - it could probably be parallel and incremental - and along not having to mine yourself for an initial database state.
Criner could upload its database once a day to an S3 bucket for instance - it's about 800MB gzipped.

# Operating Manual

## How to run migrations

As migrations are currently special purpose programs that may eat laundry for breakfast, they cannot be executed by accident.
```
RUST_LOG=info cargo run --features migration  --  migrate
```
