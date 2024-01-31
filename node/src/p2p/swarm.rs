use instant::Duration;
use libp2p::{identity::Keypair, swarm::NetworkBehaviour, Swarm, SwarmBuilder};

use crate::p2p::P2pError;

pub(crate) use self::imp::new_swarm;

#[cfg(not(target_arch = "wasm32"))]
mod imp {
    use super::*;
    use libp2p::{dns, noise, tcp, yamux};

    pub(crate) fn new_swarm<B>(keypair: Keypair, behaviour: B) -> Result<Swarm<B>, P2pError>
    where
        B: NetworkBehaviour,
    {
        Ok(SwarmBuilder::with_existing_identity(keypair)
            .with_tokio()
            .with_tcp(
                tcp::Config::default(),
                noise::Config::new,
                yamux::Config::default,
            )?
            .with_quic()
            // We do not use system's DNS because libp2p loads DNS servers only when
            // `Swarm` get constructed. This is not a problem for server machines, but
            // it is for movable machines such as laptops and smart phones. Because of
            // that, the following edge cases can happen:
            //
            // 1. Machine connects to WiFi A. Our node starts and uses DNS that
            //    WiFi A announced. Then machine moves to WiFi B, the old DNS servers
            //    become unreachable, but libp2p still uses the old ones.
            // 2. Machine is not connected to the Internet. Our node starts and does not
            //    find any DNS servers defined. Then machine connects to the Internet, but
            //    libp2p still does not have any DNS nameservers defined.
            //
            // By having a pre-defined public servers, these edge cases solved.
            .with_dns_config(
                dns::ResolverConfig::cloudflare(),
                dns::ResolverOpts::default(),
            )
            .with_behaviour(|_| behaviour)
            .expect("Moving behaviour doesn't fail")
            .with_swarm_config(|config| {
                // TODO: Refactor code to avoid being idle. This can be done by preloading a
                // handler. This is how they fixed Kademlia:
                // https://github.com/libp2p/rust-libp2p/pull/4675/files
                config.with_idle_connection_timeout(Duration::from_secs(15))
            })
            .build())
    }

    impl From<noise::Error> for P2pError {
        fn from(e: noise::Error) -> Self {
            P2pError::InitNoise(e.to_string())
        }
    }
}

#[cfg(target_arch = "wasm32")]
mod imp {
    use super::*;
    use libp2p::webtransport_websys;

    pub(crate) fn new_swarm<B>(keypair: Keypair, behaviour: B) -> Result<Swarm<B>, P2pError>
    where
        B: NetworkBehaviour,
    {
        Ok(SwarmBuilder::with_existing_identity(keypair)
            .with_wasm_bindgen()
            .with_other_transport(|local_keypair| {
                let config = webtransport_websys::Config::new(local_keypair);
                webtransport_websys::Transport::new(config)
            })
            .expect("webtransport_websys::Transport is infallible")
            .with_behaviour(|_| behaviour)
            .expect("Moving behaviour doesn't fail")
            .with_swarm_config(|config| {
                // TODO: Refactor code to avoid being idle. This can be done by preloading a
                // handler. This is how they fixed Kademlia:
                // https://github.com/libp2p/rust-libp2p/pull/4675/files
                config.with_idle_connection_timeout(Duration::from_secs(15))
            })
            .build())
    }
}
