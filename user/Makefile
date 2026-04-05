# Makefile

# Configurable variables
OUT_DIR ?=./bin
# "debug" or "release"
BUILD_MODE ?=release
TARGET ?=riscv64gc-unknown-none-elf

# Determine cargo build flags
ifeq ($(BUILD_MODE),release)
    BUILD_FLAGS := --release
else
    BUILD_FLAGS :=
endif

# Directories
BUILD_DIR := target/$(TARGET)/$(BUILD_MODE)

.PHONY: all build move clean

all: build move

build:
	cargo build $(BUILD_FLAGS)

move:
	mkdir -p $(OUT_DIR)
	# Find all executable files and move them
	find $(BUILD_DIR)/* -maxdepth 1 -type f -executable -exec cp {} $(OUT_DIR)/ \;

clean:
	cargo clean
	rm -rf $(OUT_DIR)
