use libp2p::{
    core::{muxing::StreamMuxerBox, transport::Boxed},
    identity::Keypair,
    PeerId,
};

use crate::p2p::P2pError;

pub(crate) use self::imp::new_transport;

#[cfg(not(target_arch = "wasm32"))]
mod imp {
    use super::*;
    use futures::future::Either;
    use libp2p::{
        core::{upgrade::Version, Transport},
        dns, noise, quic, tcp, yamux,
    };

    pub(crate) fn new_transport(
        keypair: &Keypair,
    ) -> Result<Boxed<(PeerId, StreamMuxerBox)>, P2pError> {
        let quic_transport = {
            let config = quic::Config::new(keypair);
            quic::tokio::Transport::new(config)
        };

        let tcp_transport = {
            let mut yamux_config = yamux::Config::default();

            // This increases bandwidth utilitation. With 1MB of receive
            // window, we can utilize 81.92mbps (10mb/s) per stream when
            // latency is 100ms: 1mb / 100ms * 8bits = 81.92mbps
            //
            // This means that machine needs 8gb of ram to handle 8192
            // streams (the default). For this reason we lower max streams
            // to 2048, so the maximum memory usage will be 2gb.
            //
            // More info: https://github.com/libp2p/rust-yamux/issues/162
            //
            // NOTE: go-libp2p sets 16mb for receive window, but they have
            // connection and memory limits in a higher layer. rust-libp2p
            // doesn't implement this, and if we used 16mb here we would be
            // vulnerable to DoS attacks.
            yamux_config.set_receive_window_size(1024 * 1024);
            yamux_config.set_max_buffer_size(1024 * 1024);
            yamux_config.set_max_num_streams(2048);

            tcp::tokio::Transport::new(tcp::Config::default())
                .upgrade(Version::V1Lazy)
                .authenticate(noise::Config::new(keypair)?)
                .multiplex(yamux_config)
        };

        let transport = quic_transport
            .or_transport(tcp_transport)
            .map(|either, _| match either {
                Either::Left((peer_id, muxer)) => (peer_id, StreamMuxerBox::new(muxer)),
                Either::Right((peer_id, muxer)) => (peer_id, StreamMuxerBox::new(muxer)),
            });

        let transport = dns::TokioDnsConfig::custom(
            transport,
            // Why not system DNS? Because if transport is constructed before user
            // connects to its router, libp2p-dns will not find any nameservers in
            // `/etc/resolv.conf` and any future DNS requests will fail even after
            // user connects.
            //
            // By having a pre-defined public server, this problem is solved.
            dns::ResolverConfig::cloudflare(),
            dns::ResolverOpts::default(),
        )
        .map_err(P2pError::InitDns)?;

        Ok(transport.boxed())
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

    pub(crate) fn new_transport(
        keypair: &Keypair,
    ) -> Result<Boxed<(PeerId, StreamMuxerBox)>, P2pError> {
        let config = webtransport_websys::Config::new(keypair);
        let transport = webtransport_websys::Transport::new(config);
        Ok(transport.boxed())
    }
}
