[![Build Status](https://travis-ci.org/crates-io/criner.svg?branch=master)](https://travis-ci.org/crates-io/criner)

### How to run migrations

As migrations are currently special purpose programs that may eat laundry for breakfast, they cannot be executed by accident.
```
RUST_LOG=info cargo run --features migration  --  migrate
```
