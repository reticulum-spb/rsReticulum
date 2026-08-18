CC ?= cc
CPPFLAGS ?=
CFLAGS ?= -O2 -g

BUILD_DIR := build
LOOPBACK := $(BUILD_DIR)/loopback.so
HOST := $(BUILD_DIR)/host

.PHONY: all check clean

all: $(LOOPBACK) $(HOST)

$(LOOPBACK): examples/loopback.c include/rns_plugin.h
	mkdir -p $(BUILD_DIR)
	$(CC) $(CPPFLAGS) $(CFLAGS) -std=c11 \
		-Wall -Wextra -Wpedantic -Werror -fPIC -fvisibility=hidden \
		-Iinclude -shared examples/loopback.c -o $@

$(HOST): examples/host.c include/rns_plugin.h
	mkdir -p $(BUILD_DIR)
	$(CC) $(CPPFLAGS) $(CFLAGS) -std=c11 -Wall -Wextra -Wpedantic -Werror \
		-Iinclude examples/host.c -ldl -o $@

check: all
	$(HOST) ./$(LOOPBACK)

clean:
	rm -f $(LOOPBACK) $(HOST)
	rmdir $(BUILD_DIR) 2>/dev/null || true
