# rsReticulum C plugin API prototype

This directory contains the ABI v1 C header and a minimal plugin used to
evaluate the API before implementing the Rust loader and adapter.

Build the example with:

```sh
make -C crates/rns-plugin
make -C crates/rns-plugin check
```

The result is `crates/rns-plugin/build/loopback.so`. The loopback
plugin supports multiple independent instances. Each successful synchronous
`send()` immediately returns the same packet through the host RX callback.
The smoke-test host loads the library with `dlopen()` and exercises two
instances with separate host contexts.

A successful `create()` returns an already-running instance. Before calling
`destroy()`, the host stops accepting TX work and joins its TX worker, so no
`send()` call is active. Plugins must use finite timeouts for hardware waits.

The host serializes only the plugin-specific `config:` YAML mapping as UTF-8
and passes it to `create()` without a terminating NUL. `NULL, 0` represents an
empty mapping. Plugins validate their own mappings and reject unknown keys.

The `plugin` value in the interface configuration is the shared-library file
name without `.so`. For example, `plugin: sx126x` resolves directly to
`/usr/lib/reticulum-rs/sx126x.so`. The host does not add a `lib` or `librns_`
prefix and does not search other directories.

Plugin names are 1 to 128 ASCII bytes and contain only `A-Z`, `a-z`, `0-9`,
`_`, or `-`; dots, path separators, and a user-supplied `.so` suffix are
invalid. Symlinks with valid names are allowed, but the loader canonicalizes
the result and verifies that its target remains inside
`/usr/lib/reticulum-rs` before loading it.

A plugin interface failure is isolated to that configured interface. A missing
file, `dlopen()` or symbol error, incompatible ABI, invalid plugin metadata, or
`create()` failure is logged with the interface name and exact library path;
the failed interface is not registered, while the process and all other
interfaces continue running.

The API exposes static plugin information (`name`, `version`, and
`description`) without creating an instance. This metadata is intended for
plugin discovery commands such as `rnsd-rs --list-plugin`. Info strings use
UTF-8 pointer-plus-length values rather than NUL termination and remain valid
while the library is loaded. All three strings are mandatory and non-empty.

`rnsd-rs --list-plugin` inspects every `.so` in the plugin directory in sorted
filename order using `RTLD_NOW | RTLD_LOCAL`. A bad or incompatible library is
shown with its error and does not stop the listing. A missing plugin directory
is an empty list. Inspected libraries remain loaded until the command exits.

No Rust panic, C++ exception, or other stack unwind may cross the C ABI in
either direction. Language bindings must catch them inside their wrappers.
Because plugins run in-process, memory corruption, signals, and `abort()` can
still terminate `rsReticulum`; the ABI does not provide process isolation.

Loaded libraries remain resident until process exit. Multiple interface
instances share one library handle but receive independent instance pointers
from `create()`. Instance shutdown calls `destroy()` without `dlclose()`; hot
reload and runtime library replacement are not supported. Discovery commands
also rely on process exit instead of explicitly unloading inspected plugins.

Calls for different instances may execute concurrently. A plugin synchronizes
library-global state and keeps each instance independent. For one instance the
host serializes `send()` calls, joins the TX worker before the single
`destroy()` call, and never uses the instance pointer afterwards.

The prototype deliberately has no Rust crate or host loader yet.
