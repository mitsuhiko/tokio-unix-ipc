# tokio-unix-ipc

This crate implements a minimal abstraction over UNIX domain sockets for
the purpose of IPC on top of tokio. It lets you send both file handles and
rust objects between processes. This is a replacement for my earlier
[unix-ipc](https://github.com/mitsuhiko/unix-ipc) crate.

## How it works

This uses [serde](https://serde.rs/) to serialize data over unix sockets
via [bincode](https://github.com/servo/bincode). Thanks to the
[`Handle`](https://docs.rs/unix-ipc/latest/unix_ipc/struct.Handle.html) abstraction you can also send any object
across that is convertable into a unix file handle.

The way this works under the hood is that during serialization and
deserialization encountered file descriptors are tracked. They are then
sent over the unix socket separately. This lets unassociated processes
share file handles.

If you only want the unix socket abstraction you can disable all default
features and use the raw channels.

## Feature Flags

All features are enabled by default but a lot can be turned off to
cut down on dependencies. With all default features enabled only
the raw types are available.

- `serde`: enables serialization and deserialization.
- `bootstrap`: adds the `Bootstrapper` type.

License: MIT/Apache-2.0
