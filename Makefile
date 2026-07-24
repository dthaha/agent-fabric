.PHONY: proto endpoint hosted test check

proto:
	buf generate

endpoint:
	cargo build --release -p fabric-endpoint

hosted:
	docker build -f deploy/docker/Dockerfile.hosted -t fabric-hosted .

test:
	cargo test --workspace

check:
	cargo clippy --workspace --all-targets -- -D warnings
	cargo fmt --all -- --check
