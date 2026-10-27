//! Compose services from the call context and dispatch one request.

use std::io::Read;
use std::time::SystemTime;

use office_core::ports::{Clock, Library, Models, Orbit, Ports};
use office_core::protocol::{self, Context};

use crate::error::Result;

// These handles retain only the supplied context; their adapters arrive with
// the first port operations. In particular, constructing a Library does no I/O.
struct ContextHandle<'a> {
    #[expect(dead_code, reason = "adapter operations will use the call context")]
    context: &'a Context,
}
impl Library for ContextHandle<'_> {}
impl Models for ContextHandle<'_> {}
impl Orbit for ContextHandle<'_> {}
struct SystemClock;
impl Clock for SystemClock {
    fn now(&self) -> SystemTime {
        SystemTime::now()
    }
}

fn ports(context: &Context) -> Ports<'_> {
    Ports {
        library: Box::new(ContextHandle { context }),
        models: Box::new(ContextHandle { context }),
        orbit: Box::new(ContextHandle { context }),
        clock: Box::new(SystemClock),
    }
}

/// Serve one Orbit exec call and return the payload for stdout.
pub(crate) fn serve_exec() -> Result<String> {
    let mut raw = String::new();
    let _ = std::io::stdin().read_to_string(&mut raw);
    Ok(match protocol::parse(&raw) {
        Ok(request) => office_tools::serve(&request, &ports(&request.context)),
        Err(error) => protocol::respond(Err(error)),
    })
}
