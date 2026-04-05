# Makefile

.PHONY: all user mkfs

all: user mkfs

user:
	$(MAKE) -C user

mkfs:
	cd mkfs && cargo run -- -b ../user/bin -f -m ./fs.img
