PLAT ?= qemu-virt-riscv
HOST_TARGET ?= x86_64-pc-windows-msvc

.PHONY: all config menuconfig build run clean

all: build

config:
	cargo run -p xtask --release --target $(HOST_TARGET) -- --host-target $(HOST_TARGET) config

menuconfig:
	cargo run -p xtask --release --target $(HOST_TARGET) -- --host-target $(HOST_TARGET) menuconfig

build:
	cargo run -p xtask --release --target $(HOST_TARGET) -- --host-target $(HOST_TARGET) build --platform $(PLAT)

run:
	cargo run -p xtask --release --target $(HOST_TARGET) -- --host-target $(HOST_TARGET) run --platform $(PLAT)

clean:
	cargo clean
