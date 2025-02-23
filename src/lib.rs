//! # tokio-unix-ipc
//!
//! This crate implements a minimal abstraction over UNIX domain sockets for
//! the purpose of IPC on top of tokio. It lets you send both file handles and
//! rust objects between processes. This is a replacement for my earlier
//! [unix-ipc](https://github.com/mitsuhiko/unix-ipc) crate.
//!
//! ## How it works
//!
//! This uses [serde](https://serde.rs/) to serialize data over unix sockets
//! via [bincode](https://github.com/bincode-org/bincode). Thanks to the
//! [`Handle`](crate::serde::Handle) abstraction you can also send any object
//! across that is convertable into a unix file handle.
//!
//! The way this works under the hood is that during serialization and
//! deserialization encountered file descriptors are tracked. They are then
//! sent over the unix socket separately. This lets unassociated processes
//! share file handles.
//!
//! If you only want the unix socket abstraction you can disable all default
//! features and use the raw channels.
//!
//! ## Features
//!
//! * `serde`: enables support for the non raw types
//! * `serde-structural`: enables support for structural data passing
//! * `bootstrap`: adds support for boostrapping
//! * `panic-support`: adds support for IPC panic catching
//! * `backtrace`: adds support for backtraces to panics

// because of PanicInfo
#![allow(deprecated)]

mod raw_channel;

#[cfg(feature = "serde")]
pub mod serde;

#[cfg(feature = "serde")]
mod typed_channel;

#[cfg(feature = "panic-support")]
pub mod panic;

#[cfg(feature = "bootstrap")]
mod bootstrap;

pub use self::raw_channel::*;

#[cfg(feature = "serde")]
pub use self::typed_channel::*;

#[cfg(feature = "bootstrap")]
pub use self::bootstrap::*;

#[doc(hidden)]
#[cfg(feature = "serde")]
pub use ::serde_ as _serde_ref;
