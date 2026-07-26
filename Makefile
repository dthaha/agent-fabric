.PHONY: proto endpoint endpoint-cross server test check eval-decoder

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

server:
	docker build -f deploy/docker/Dockerfile.server -t fabric-server .

test:
	cargo test --workspace

check:
	cargo clippy --workspace --all-targets -- -D warnings
	cargo fmt --all -- --check

# Live conflict-decoder eval against a real endpoint. Opt-in; requires
# OPENAI_BASE_URL + FABRIC_DECODER_MODEL (OPENAI_API_KEY if the endpoint
# needs auth). Not run in CI — the DRY eval runs in `make test` instead.
eval-decoder:
	cargo test -p fabric-context --test decoder_eval live -- --ignored --nocapture
