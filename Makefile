.PHONY: proto endpoint hosted test check

proto:
	buf generate
	# prost-serde emits serde impls for google.protobuf WKT even though
	# extern_path maps them to pbjson-types (which already implements serde).
	# The output would violate the orphan rule if included; drop it.
	rm -rf core/types/src/gen/google

endpoint:
	cargo build --release -p fabric-endpoint

hosted:
	docker build -f deploy/docker/Dockerfile.hosted -t fabric-hosted .

test:
	cargo test --workspace

check:
	cargo clippy --workspace --all-targets -- -D warnings
	cargo fmt --all -- --check
