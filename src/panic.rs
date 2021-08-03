//! Support for cross-process panic handling.
//!
//! This module provides a version of the standard library's
//! panic info object ([`PanicInfo`]) which can hold information
//! about why a panic happened so it can be sent across processes.
//!
//! To capture panics instead of crashing the panic hook needs to
//! be installed ([`init_panic_hook`]) and afterwards the panicking
//! function needs to be invoked via [`catch_panic`].
use std::any::Any;
use std::cell::RefCell;
use std::fmt;
use std::panic;

use serde_::{Deserialize, Serialize};

fn serialize_panic(panic: &(dyn Any + Send + 'static)) -> PanicInfo {
    PanicInfo::new(match panic.downcast_ref::<&'static str>() {
        Some(s) => *s,
        None => match panic.downcast_ref::<String>() {
            Some(s) => &s[..],
            None => "Box<Any>",
        },
    })
}

/// Represents a panic caugh across processes.
///
/// This contains the marshalled panic information so that it can be used
/// for other purposes.
///
/// This is similar to [`std::panic::PanicInfo`] but can cross process boundaries.
#[derive(Serialize, Deserialize)]
#[serde(crate = "serde_")]
pub struct PanicInfo {
    msg: String,
    pub(crate) location: Option<Location>,
    #[cfg(feature = "backtrace")]
    pub(crate) backtrace: Option<backtrace::Backtrace>,
}

/// Location of a panic.
///
/// This is similar to `std::panic::Location` but can cross process boundaries.
#[derive(Serialize, Deserialize, Debug)]
#[serde(crate = "serde_")]
pub struct Location {
    file: String,
    line: u32,
    column: u32,
}

impl Location {
    fn from_std(loc: &std::panic::Location) -> Location {
        Location {
            file: loc.file().into(),
            line: loc.line(),
            column: loc.column(),
        }
    }

    /// Returns the name of the source file from which the panic originated.
    pub fn file(&self) -> &str {
        &self.file
    }

    /// Returns the line number from which the panic originated.
    pub fn line(&self) -> u32 {
        self.line
    }

    /// Returns the column from which the panic originated.
    pub fn column(&self) -> u32 {
        self.column
    }
}

impl PanicInfo {
    /// Creates a new panic object.
    pub(crate) fn new(s: &str) -> PanicInfo {
        PanicInfo {
            msg: s.into(),
            location: None,
            #[cfg(feature = "backtrace")]
            backtrace: None,
        }
    }

    /// Creates a panic info from a standard library one.
    ///
    /// It will attempt to extract all information available from it
    /// and if `capture_backtrace` is set to `true` it will also
    /// capture a backtrace at the current location and assign it
    /// to the panic info object.
    ///
    /// For the backtrace parameter to have an effect the
    /// `backtrace` feature needs to be enabled.
    pub fn from_std(info: &std::panic::PanicInfo, capture_backtrace: bool) -> PanicInfo {
        #[allow(unused_mut)]
        let mut panic = serialize_panic(info.payload());
        #[cfg(feature = "backtrace")]
        {
            if capture_backtrace {
                panic.backtrace = Some(backtrace::Backtrace::new());
            }
        }
        #[cfg(not(feature = "backtrace"))]
        {
            let _ = capture_backtrace;
        }
        panic.location = info.location().map(Location::from_std);
        panic
    }

    /// Returns the message of the panic.
    pub fn message(&self) -> &str {
        self.msg.as_str()
    }

    /// Returns the panic location.
    pub fn location(&self) -> Option<&Location> {
        self.location.as_ref()
    }

    /// Returns a reference to the backtrace.
    ///
    /// Typically this backtrace is already resolved because it's currently
    /// not possible to cross the process boundaries with unresolved backtraces.
    #[cfg(feature = "backtrace")]
    pub fn backtrace(&self) -> Option<&backtrace::Backtrace> {
        self.backtrace.as_ref()
    }
}

impl fmt::Debug for PanicInfo {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("PanicInfo")
            .field("message", &self.message())
            .field("location", &self.location())
            .field("backtrace", &{
                #[cfg(feature = "backtrace")]
                {
                    self.backtrace()
                }
                #[cfg(not(feature = "backtrace"))]
                {
                    None::<()>
                }
            })
            .finish()
    }
}

impl fmt::Display for PanicInfo {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.msg)
    }
}

thread_local! {
    static PANIC_INFO: RefCell<Option<PanicInfo>> = RefCell::new(None);
}

fn reset_panic_info() {
    PANIC_INFO.with(|pi| {
        *pi.borrow_mut() = None;
    });
}

fn take_panic_info(payload: &(dyn Any + Send + 'static)) -> PanicInfo {
    PANIC_INFO
        .with(|pi| pi.borrow_mut().take())
        .unwrap_or_else(move || serialize_panic(payload))
}

fn panic_handler(info: &panic::PanicInfo<'_>, capture_backtrace: bool) {
    PANIC_INFO.with(|pi| {
        *pi.borrow_mut() = Some(PanicInfo::from_std(info, capture_backtrace));
    });
}

/// Invokes a function and captures the panic.
///
/// The captured panic info will only be fully filled if the panic hook
/// has been installed (see [`init_panic_hook`]).
pub fn catch_panic<F: FnOnce() -> R, R>(func: F) -> Result<R, PanicInfo> {
    reset_panic_info();
    match panic::catch_unwind(panic::AssertUnwindSafe(|| func())) {
        Ok(rv) => Ok(rv),
        Err(panic) => Err(take_panic_info(&*panic)),
    }
}

/// Initializes the panic hook for IPC usage.
///
/// When enabled the default panic handler is disabled and panics are stored in a
/// thread local storage. This needs to be called for the [`catch_panic`]
/// function in this module to provide the correct panic info.
pub fn init_panic_hook(capture_backtraces: bool) {
    let next = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
        panic_handler(info, capture_backtraces);
        next(info);
    }));
}

#[test]
fn test_panic_hook() {
    init_panic_hook(true);
    let rv = catch_panic(|| {
        panic!("something went wrong");
    });
    let pi = rv.unwrap_err();
    assert_eq!(pi.message(), "something went wrong");
    #[cfg(feature = "backtrace")]
    {
        let bt = format!("{:?}", pi.backtrace().unwrap());
        assert!(bt.contains("PanicInfo::from_std"));
    }
}
