# 默认平台
PLAT ?= qemu-virt-riscv
# PLAT ?= qemu-an521

# 配置平台对应的包名和目标架构
ifeq ($(PLAT), qemu-virt-riscv)
	PKG = qemu-virt-riscv
	TARGET = riscv64imac-unknown-none-elf
	# 使用指定的 rustsbi 固件，并保留 -kernel 以便 QEMU 传递设备树给固件
	QEMU_CMD = qemu-system-riscv64 -machine virt -smp 1 -nographic \
	-bios platform/qemu-virt-riscv/rustsbi-prototyper-jump.elf \
	-kernel target/$(TARGET)/debug/$(PKG) \
	-global virtio-mmio.force-legacy=false \
    -device virtio-blk-device,drive=hd0 \
    -drive if=none,file=sd.img,format=raw,id=hd0 \
	-s
else ifeq ($(PLAT), qemu-an521)
	PKG = an521
	TARGET = thumbv8m.main-none-eabihf
	QEMU_CMD = qemu-system-arm -M mps2-an521 -nographic -kernel target/$(TARGET)/debug/$(PKG)
else
	$(error "Unknown Platform: $(PLAT)")
endif

.PHONY: all build run clean

all: build

build:
	cargo build -p $(PKG) --target $(TARGET)

run: build
	$(QEMU_CMD)

clean:
	cargo clean

