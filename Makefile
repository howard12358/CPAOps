.DEFAULT_GOAL := help

.PHONY: help build release test check smoke clean

help:
	@printf '%s\n' '可用目标：'
	@printf '%s\n' '  make build    调试构建 cpactl'
	@printf '%s\n' '  make release  发布构建 cpactl'
	@printf '%s\n' '  make test     运行全部 Rust 测试'
	@printf '%s\n' '  make check    格式、Clippy 与测试检查'
	@printf '%s\n' '  make smoke    运行隔离的 macOS 烟测'
	@printf '%s\n' '  make clean    清理 Cargo 构建产物'

build:
	cargo build

release:
	cargo build --release

test:
	cargo test

check:
	cargo fmt --check
	cargo clippy --all-targets -- -D warnings
	cargo test

smoke:
	sh tests/smoke/macos.sh

clean:
	cargo clean
