.PHONY: user

# Compile user programs in src/bin
user: user/hello

user/% : kernel/src/bin/%.rs makefile
	cargo +nightly rustc --release -p kernel --bin $* -- \
		-Zbuild-std=core,alloc -Zbuild-std-features=panic_immediate_abort \
		-C linker-flavor=ld \
		-C link-args="-Ttext-segment=5000000 -Trodata-segment=5100000" \
		-C relocation-model=static
	mkdir -p user
	cp target/x86_64-unknown-none/release/$* user/