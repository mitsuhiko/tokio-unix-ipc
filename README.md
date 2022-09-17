# tokio-unix-ipc

[![Build Status](https://github.com/mitsuhiko/tokio-unix-ipc/workflows/Tests/badge.svg?branch=main)](https://github.com/mitsuhiko/tokio-unix-ipc/actions?query=workflow%3ATests)
[![Crates.io](https://img.shields.io/crates/d/tokio-unix-ipc.svg)](https://crates.io/crates/tokio-unix-ipc)
[![License](https://img.shields.io/github/license/mitsuhiko/tokio-unix-ipc)](https://github.com/mitsuhiko/tokio-unix-ipc/blob/main/LICENSE)
[![rustc 1.53.0](https://img.shields.io/badge/rust-1.53%2B-orange.svg)](https://img.shields.io/badge/rust-1.53%2B-orange.svg)
[![Documentation](https://docs.rs/tokio-unix-ipc/badge.svg)](https://docs.rs/tokio-unix-ipc)

This crate implements a minimal abstraction over UNIX domain sockets for
the purpose of IPC on top of tokio. It lets you send both file handles and
rust objects between processes. This is a replacement for my earlier
[unix-ipc](https://github.com/mitsuhiko/unix-ipc) crate.

## How it works

This uses [serde](https://serde.rs/) to serialize data over unix sockets
via [bincode](https://github.com/bincode-org/bincode). Thanks to the
`Handle` abstraction you can also send any object
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

## License and Links

* [Documentation](https://docs.rs/tokio-unix-ipc/)
* [Issue Tracker](https://github.com/mitsuhiko/tokio-unix-ipc/issues)
* [Examples](https://github.com/mitsuhiko/tokio-unix-ipc/tree/main/examples)
* License: [Apache-2.0](https://github.com/mitsuhiko/tokio-unix-ipc/blob/main/LICENSE)
