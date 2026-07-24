.PHONY: proto endpoint endpoint-cross hosted test check

proto:
	buf generate
	# prost-serde emits serde impls for google.protobuf WKT even though
	# extern_path maps them to pbjson-types (which already implements serde).
	# The output would violate the orphan rule if included; drop it.
	rm -rf core/types/src/gen/google

endpoint:
	cargo build --release -p fabric-endpoint

# Local cross-compile targets; not run in CI.
endpoint-cross:
	cargo build --release -p fabric-endpoint --target aarch64-apple-darwin
	cargo build --release -p fabric-endpoint --target x86_64-apple-darwin
	cargo build --release -p fabric-endpoint --target x86_64-unknown-linux-gnu
	cargo build --release -p fabric-endpoint --target aarch64-unknown-linux-gnu

hosted:
	docker build -f deploy/docker/Dockerfile.hosted -t fabric-hosted .

test:
	cargo test --workspace

check:
	cargo clippy --workspace --all-targets -- -D warnings
	cargo fmt --all -- --check
