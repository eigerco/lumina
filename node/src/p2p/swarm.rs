use libp2p::{identity::Keypair, swarm::NetworkBehaviour, Swarm, SwarmBuilder};
use web_time::Duration;

use crate::p2p::P2pError;

pub(crate) use self::imp::new_swarm;

#[cfg(not(target_arch = "wasm32"))]
mod imp {
    use std::env;

    use libp2p::core::transport::OptionalTransport;
    use libp2p::core::upgrade::Version;
    use libp2p::{dns, noise, tcp, websocket, yamux, Transport};
    use tokio::fs;

    use super::*;

    pub(crate) async fn new_swarm<B>(keypair: Keypair, behaviour: B) -> Result<Swarm<B>, P2pError>
    where
        B: NetworkBehaviour,
    {
        let cert_file = env::var("LUMINA_WSS_CERT_DER").ok();
        let key_file = env::var("LUMINA_WSS_KEY_DER").ok();

        let maybe_wss_transport = if let (Some(cert_file), Some(key_file)) = (cert_file, key_file) {
            let mut wss_transport = websocket::WsConfig::new(dns::tokio::Transport::custom(
                tcp::tokio::Transport::new(tcp::Config::default()),
                dns::ResolverConfig::cloudflare(),
                dns::ResolverOpts::default(),
            ));

            let pk = fs::read(key_file).await.expect("key");
            let cert = fs::read(cert_file).await.expect("cert");
            let pk = websocket::tls::PrivateKey::new(pk);
            let cert = websocket::tls::Certificate::new(cert);
            wss_transport.set_tls_config(websocket::tls::Config::new(pk, vec![cert]).expect("tls"));

            OptionalTransport::some(
                wss_transport
                    .upgrade(Version::V1)
                    .authenticate(noise::Config::new(&keypair).unwrap())
                    .multiplex(yamux::Config::default()),
            )
        } else {
            OptionalTransport::none()
        };

        Ok(SwarmBuilder::with_existing_identity(keypair)
            .with_tokio()
            .with_tcp(
                tcp::Config::default(),
                noise::Config::new,
                yamux::Config::default,
            )?
            .with_quic()
            .with_other_transport(|_| maybe_wss_transport)
            .expect("passing contstructed tranpsort is infallible")
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
    use libp2p::core::upgrade::Version;
    use libp2p::{noise, websocket_websys, webtransport_websys, yamux, Transport};

    pub(crate) async fn new_swarm<B>(keypair: Keypair, behaviour: B) -> Result<Swarm<B>, P2pError>
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
            .with_other_transport(|local_keypair| {
                let noise_config = noise::Config::new(local_keypair)?;
                Ok(websocket_websys::Transport::default()
                    .upgrade(Version::V1)
                    .authenticate(noise_config)
                    .multiplex(yamux::Config::default()))
            })
            .map_err(|_| P2pError::InitNoise("websocket-websys".to_string()))?
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
