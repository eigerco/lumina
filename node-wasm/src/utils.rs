//! Various utilities for interacting with node from wasm.
use std::borrow::Cow;
use std::fmt::{self, Debug};
use std::future::Future;
use std::net::{IpAddr, Ipv4Addr};

use gloo_timers::future::TimeoutFuture;
use js_sys::{Math, Promise};
use libp2p::multiaddr::Protocol;
use libp2p::{Multiaddr, PeerId};
use serde::{Deserialize, Serialize};
use serde_repr::{Deserialize_repr, Serialize_repr};
use serde_wasm_bindgen::Serializer;
use tracing::{info, warn};
use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::fmt::time::UtcTime;
use tracing_subscriber::prelude::*;
use tracing_web::MakeConsoleWriter;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;
use web_sys::{
    DedicatedWorkerGlobalScope, MessageEvent, Request, RequestInit, RequestMode, Response,
    ServiceWorker, ServiceWorkerGlobalScope, SharedWorker, SharedWorkerGlobalScope, Worker,
};

use lumina_node::network;

use crate::error::{Context, Error, Result};

/// Supported Celestia networks.
#[wasm_bindgen]
#[derive(PartialEq, Eq, Clone, Copy, Serialize_repr, Deserialize_repr, Debug)]
#[repr(u8)]
pub enum Network {
    /// Celestia mainnet.
    Mainnet,
    /// Arabica testnet.
    Arabica,
    /// Mocha testnet.
    Mocha,
    /// Private local network.
    Private,
}

/// Set up a logging layer that direct logs to the browser's console.
#[wasm_bindgen(start)]
pub fn setup_logging() {
    console_error_panic_hook::set_once();

    let fmt_layer = tracing_subscriber::fmt::layer()
        .with_ansi(false)
        .with_timer(UtcTime::rfc_3339()) // std::time is not available in browsers
        .with_writer(MakeConsoleWriter) // write events to the console
        .with_filter(LevelFilter::INFO); // TODO: allow customizing the log level

    tracing_subscriber::registry().with(fmt_layer).init();
}

impl From<Network> for network::Network {
    fn from(network: Network) -> network::Network {
        match network {
            Network::Mainnet => network::Network::Mainnet,
            Network::Arabica => network::Network::Arabica,
            Network::Mocha => network::Network::Mocha,
            Network::Private => network::Network::custom("private").expect("invalid network id"),
        }
    }
}

impl TryFrom<network::Network> for Network {
    type Error = Error;

    fn try_from(network: network::Network) -> Result<Network, Error> {
        match network {
            network::Network::Mainnet => Ok(Network::Mainnet),
            network::Network::Arabica => Ok(Network::Arabica),
            network::Network::Mocha => Ok(Network::Mocha),
            network::Network::Custom(id) => match id.as_ref() {
                "private" => Ok(Network::Private),
                _ => Err(Error::new("Unsupported network id: {id}")),
            },
        }
    }
}

pub(crate) fn js_value_from_display<D: fmt::Display>(value: D) -> JsValue {
    JsValue::from(value.to_string())
}

trait WorkerSelf {
    type GlobalScope: JsCast;

    fn worker_self() -> Self::GlobalScope {
        js_sys::global().unchecked_into()
    }

    fn is_worker_type() -> bool {
        js_sys::global().has_type::<Self::GlobalScope>()
    }
}

impl WorkerSelf for SharedWorker {
    type GlobalScope = SharedWorkerGlobalScope;
}

impl WorkerSelf for Worker {
    type GlobalScope = DedicatedWorkerGlobalScope;
}

impl WorkerSelf for ServiceWorker {
    type GlobalScope = ServiceWorkerGlobalScope;
}

pub(crate) trait MessageEventExt {
    fn get_port(&self) -> Option<JsValue>;
}
impl MessageEventExt for MessageEvent {
    fn get_port(&self) -> Option<JsValue> {
        let ports = self.ports();
        if ports.is_array() {
            let port = ports.get(0);
            if !port.is_undefined() {
                return Some(port);
            }
        }
        None
    }
}

/// Request persistent storage from user for us, which has side effect of increasing the quota we
/// have. This function doesn't `await` on JavaScript promise, as that would block until user
/// either allows or blocks our request in a prompt (and we cannot do much with the result anyway).
pub(crate) async fn request_storage_persistence() -> Result<(), Error> {
    let storage_manager = if let Some(window) = web_sys::window() {
        window.navigator().storage()
    } else if Worker::is_worker_type() {
        Worker::worker_self().navigator().storage()
    } else if SharedWorker::is_worker_type() {
        SharedWorker::worker_self().navigator().storage()
    } else if ServiceWorker::is_worker_type() {
        warn!("ServiceWorker doesn't have access to StorageManager");
        return Ok(());
    } else {
        return Err(Error::new("`navigator.storage` not found in global scope"));
    };

    let fullfiled = Closure::once(move |granted: JsValue| {
        if granted.is_truthy() {
            info!("Storage persistence acquired: {:?}", granted);
        } else {
            warn!("User rejected storage persistance request")
        }
    });
    let rejected = Closure::once(move |_ev: JsValue| {
        warn!("Error during persistant storage request");
    });

    // don't drop the promise, we'll log the result and hope the user clicked the right button
    let _promise = storage_manager.persist()?.then2(&fullfiled, &rejected);

    // stop rust from dropping them
    fullfiled.forget();
    rejected.forget();

    Ok(())
}

const CHROME_USER_AGENT_DETECTION_STR: &str = "Chrome/";
const FIREFOX_USER_AGENT_DETECTION_STR: &str = "Firefox/";
const SAFARI_USER_AGENT_DETECTION_STR: &str = "Safari/";

pub(crate) fn get_user_agent() -> Result<String, Error> {
    if let Some(window) = web_sys::window() {
        Ok(window.navigator().user_agent()?)
    } else if Worker::is_worker_type() {
        Ok(Worker::worker_self().navigator().user_agent()?)
    } else if SharedWorker::is_worker_type() {
        Ok(SharedWorker::worker_self().navigator().user_agent()?)
    } else if ServiceWorker::is_worker_type() {
        Ok(ServiceWorker::worker_self().navigator().user_agent()?)
    } else {
        Err(Error::new(
            "`navigator.user_agent` not found in global scope",
        ))
    }
}

#[allow(dead_code)]
pub(crate) fn is_chrome() -> Result<bool, Error> {
    let user_agent = get_user_agent()?;
    Ok(user_agent.contains(CHROME_USER_AGENT_DETECTION_STR))
}

#[allow(dead_code)]
pub(crate) fn is_firefox() -> Result<bool, Error> {
    let user_agent = get_user_agent()?;
    Ok(user_agent.contains(FIREFOX_USER_AGENT_DETECTION_STR))
}

pub(crate) fn is_safari() -> Result<bool, Error> {
    let user_agent = get_user_agent()?;
    // Chrome contains `Safari/`, so make sure user agent doesn't contain `Chrome/`
    Ok(user_agent.contains(SAFARI_USER_AGENT_DETECTION_STR)
        && !user_agent.contains(CHROME_USER_AGENT_DETECTION_STR))
}

#[allow(dead_code)]
pub(crate) fn shared_workers_supported() -> Result<bool, Error> {
    // For chrome we default to running in a dedicated Worker because:
    // 1. Chrome Android does not support SharedWorkers at all
    // 2. On desktop Chrome, restarting Lumina's worker causes all network connections to fail.
    Ok(is_firefox()? || is_safari()?)
}

pub(crate) fn random_id() -> u32 {
    (Math::random() * f64::from(u32::MAX)).floor() as u32
}

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_name = fetch)]
    fn fetch_with_request(input: &Request) -> Promise;
}

async fn fetch(url: &str, opts: &RequestInit, headers: &[(&str, &str)]) -> Result<Response, Error> {
    let request = Request::new_with_str_and_init(url, opts)
        .with_context(|| format!("failed to create a request to {url}"))?;

    for (name, value) in headers {
        request
            .headers()
            .set(name, value)
            .with_context(|| format!("failed setting header: '{name}: {value}'"))?;
    }

    let fetch_promise = fetch_with_request(&request);

    JsFuture::from(fetch_promise)
        .await
        .with_context(|| format!("failed fetching {url}"))?
        .dyn_into()
        .context("`response` is not `Response` type")
}

/// If provided multiaddress uses dnsaddr protocol, resolve it using dns-over-https.
/// Otherwise returns the provided address.
pub(crate) async fn resolve_dnsaddr_multiaddress(ma: Multiaddr) -> Result<Vec<Multiaddr>> {
    const TXT_TYPE: u16 = 16;
    // cloudflare dns
    const DEFAULT_DNS_ADDR: IpAddr = IpAddr::V4(Ipv4Addr::new(1, 1, 1, 1));

    #[derive(Debug, Deserialize)]
    struct DohEntry {
        r#type: u16,
        data: String,
    }

    #[derive(Debug, Deserialize)]
    #[serde(rename_all = "PascalCase")]
    struct DohResponse {
        answer: Vec<DohEntry>,
    }

    let Some(dnsaddr) = get_dnsaddr(&ma) else {
        // not a dnsaddr multiaddr
        return Ok(vec![ma]);
    };
    let Some(peer_id) = get_peer_id(&ma) else {
        return Err(Error::new("Peer id not found"));
    };

    let opts = RequestInit::new();
    opts.set_method("GET");
    opts.set_mode(RequestMode::Cors);

    let url =
        format!("https://{DEFAULT_DNS_ADDR}/dns-query?type={TXT_TYPE}&name=_dnsaddr.{dnsaddr}");
    let response = fetch(&url, &opts, &[("Accept", "application/dns-json")]).await?;

    let json_promise = response.json().context("`Response::json()` failed")?;
    let json = JsFuture::from(json_promise)
        .await
        .context("failed parsing response as json")?;

    let doh_response: DohResponse = serde_wasm_bindgen::from_value(json)
        .context("failed deserializing dns-over-https response")?;

    let mut resolved_addrs = Vec::with_capacity(3);
    for entry in doh_response.answer {
        if entry.r#type == TXT_TYPE {
            // we receive data as json encoded strings in this format:
            // "data": "\"dnsaddr=/dns/da-bridge-1.celestia-arabica-11.com/tcp/2121/p2p/12D3KooWGqwzdEqM54Dce6LXzfFr97Bnhvm6rN7KM7MFwdomfm4S\""
            let Ok(data) = serde_json::from_str::<String>(&entry.data) else {
                continue;
            };
            let Some((_, ma)) = data.split_once('=') else {
                continue;
            };
            let Ok(ma) = ma.parse() else {
                continue;
            };
            // only take results with the same peer id
            if Some(peer_id) == get_peer_id(&ma) {
                // TODO: handle recursive dnsaddr queries
                resolved_addrs.push(ma);
            }
        }
    }

    Ok(resolved_addrs)
}

fn get_peer_id(ma: &Multiaddr) -> Option<PeerId> {
    ma.iter().find_map(|protocol| {
        if let Protocol::P2p(peer_id) = protocol {
            Some(peer_id)
        } else {
            None
        }
    })
}

fn get_dnsaddr(ma: &Multiaddr) -> Option<Cow<'_, str>> {
    ma.iter().find_map(|protocol| {
        if let Protocol::Dnsaddr(addr) = protocol {
            Some(addr)
        } else {
            None
        }
    })
}

pub(crate) async fn timeout<F: Future>(millis: u32, fut: F) -> Result<F::Output, ()> {
    let timeout = TimeoutFuture::new(millis);
    tokio::select! {
        _ = timeout => Err(()),
        res = fut => Ok(res),
    }
}

pub(crate) fn to_json_value<T: Serialize + ?Sized>(
    value: &T,
) -> Result<JsValue, serde_wasm_bindgen::Error> {
    value.serialize(&Serializer::json_compatible())
}
