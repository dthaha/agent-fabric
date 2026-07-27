.PHONY: proto endpoint endpoint-cross server test check eval-decoder eval-mediator

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

# Live conflict-decoder eval against a real endpoint. Opt-in; pre-wired to
# Nemotron 3 Nano via OpenRouter (reasoning OFF via extra_body, classify-
# tuned sampling). Requires OPENAI_BASE_URL (OPENAI_API_KEY if the endpoint
# needs auth). Not run in CI — the DRY eval runs in `make test` instead.
eval-decoder:
	FABRIC_DECODER_MODEL=nvidia/nemotron-3-nano-30b-a3b \
	FABRIC_DECODER_TEMPERATURE=0.1 \
	FABRIC_DECODER_TOP_P=0.9 \
	FABRIC_DECODER_MAX_TOKENS=300 \
	FABRIC_DECODER_TIMEOUT_MS=30000 \
	FABRIC_DECODER_EXTRA_BODY={\"reasoning\":{\"effort\":\"none\"},\"provider\":{\"sort\":\"throughput\"},\"top_k\":20} \
	cargo test -p fabric-context --test decoder_eval live -- --ignored --nocapture

# Live conflict-mediator eval against a real endpoint. Opt-in; pre-wired to
# Nemotron 3 Nano via OpenRouter (reasoning ON via extra_body, mediation-
# tuned sampling). Requires OPENAI_BASE_URL (OPENAI_API_KEY if the endpoint
# needs auth). Not run in CI — the DRY eval runs in `make test` instead.
eval-mediator:
	FABRIC_MEDIATOR_MODEL=nvidia/nemotron-3-nano-30b-a3b \
	FABRIC_MEDIATOR_TEMPERATURE=0.7 \
	FABRIC_MEDIATOR_TOP_P=0.9 \
	FABRIC_MEDIATOR_MAX_TOKENS=2048 \
	FABRIC_MEDIATOR_TIMEOUT_MS=120000 \
	FABRIC_MEDIATOR_EXTRA_BODY={\"reasoning\":{\"effort\":\"high\"},\"provider\":{\"sort\":\"throughput\"},\"top_k\":20} \
	cargo test -p fabric-context --test mediator_eval live -- --ignored --nocapture
