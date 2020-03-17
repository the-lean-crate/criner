.PHONY : tests build

help:  ## Display this help
	@awk 'BEGIN {FS = ":.*##"; printf "\nUsage:\n  make \033[36m<target>\033[0m\n"} /^[a-zA-Z_-]+:.*?##/ { printf "  \033[36m%-15s\033[0m %s\n", $$1, $$2 } /^##@/ { printf "\n\033[1m%s\033[0m\n", substr($$0, 5) } ' $(MAKEFILE_LIST)


EXECUTABLE = target/debug/criner
RUST_SRC_FILES = $(shell find src -name "*.rs")
bare_index_path = index-bare

$(bare_index_path):
	mkdir -p $(dir $@)
	git clone --bare https://github.com/rust-lang/crates.io-index $@

$(EXECUTABLE): $(RUST_SRC_FILES)
	cargo build --all-features

sloc: ## Count lines of code, without tests
	tokei -e '*_test*'

##@ Dataset

crates-io-db-dump.tar.gz:
	curl --progress https://static.crates.io/db-dump.tar.gz > $@

update-crate-db: crates-io-db-dump.tar.gz ## Pull all DB data from crates.io - updated every 24h

##@ Testing

run: $(EXECUTABLE) ## Run the CLI with user interface
	$(EXECUTABLE)

tests: ## Run all tests we have (NONE for now, we just build things)
	cargo check --all --examples

