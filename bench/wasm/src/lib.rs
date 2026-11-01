//! The stdio loop of a WASI plugin: JSON-RPC 2.0, one message per line, the
//! same protocol every plugin speaks (docs/contract.md). It runs a module
//! written against the `Module` trait.

use std::future::Future;
use std::io::{BufRead, Write};
use std::pin::pin;
use std::task::{Context, Poll, Waker};

use dashboard_contract::rpc::{InitializeParams, InitializeResult, Request, Response, RpcError};
use dashboard_contract::rpc::{error_code, method};
use dashboard_host::Module;
use serde_json::Value;

/// Answer requests on stdin until it closes.
pub fn serve(mut module: impl Module) {
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        // A notification has no id, so it does not parse as a Request and
        // gets no reply, as JSON-RPC asks.
        let Ok(req) = serde_json::from_str::<Request>(&line) else {
            continue;
        };
        let reply = match req.method.as_str() {
            method::INITIALIZE => {
                let params: Option<InitializeParams> =
                    req.params.and_then(|p| serde_json::from_value(p).ok());
                let config = params.map(|p| p.config).unwrap_or(Value::Null);
                block_on(module.init(config))
                    .map(|manifest| serde_json::to_value(InitializeResult { manifest }).unwrap())
                    .map_err(|e| error(error_code::CONFIG_INVALID, e))
            }
            method::VIEW_RENDER => block_on(module.render())
                .map(|view| serde_json::to_value(view).unwrap())
                .map_err(|e| error(error_code::UPSTREAM_UNAVAILABLE, e)),
            method::SHUTDOWN => Ok(Value::Null),
            other => Err(RpcError {
                code: error_code::METHOD_NOT_FOUND,
                message: format!("unknown method {other}"),
                data: None,
            }),
        };
        let (result, error) = match reply {
            Ok(v) => (Some(v), None),
            Err(e) => (None, Some(e)),
        };
        let resp = Response {
            jsonrpc: "2.0".into(),
            id: req.id,
            result,
            error,
        };
        let _ = writeln!(stdout, "{}", serde_json::to_string(&resp).unwrap());
        let _ = stdout.flush();
    }
}

fn error(code: i64, err: anyhow::Error) -> RpcError {
    RpcError {
        code,
        message: format!("{err:#}"),
        data: None,
    }
}

/// Run a future that never waits on anything (these modules only do file
/// reads and math), with no async runtime.
fn block_on<T>(fut: impl Future<Output = T>) -> T {
    let mut fut = pin!(fut);
    let mut cx = Context::from_waker(Waker::noop());
    loop {
        if let Poll::Ready(v) = fut.as_mut().poll(&mut cx) {
            return v;
        }
    }
}
