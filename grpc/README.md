# Celestia gRPC

A collection of types for interacting with Celestia validator nodes over gRPC

This crate builds on top of [`tonic`](https://docs.rs/tonic).

## TLS support

Client will be configured to use TLS if at least one of the trust roots provider is enabled using the crate features
`tls-native-roots` and `tls-webpki-roots`. The trust roots are additive, selecting both will result in both being in use.

Moreover, the crypto provider for `rustls` can be configured by using `tls-ring` and `tls-aws-lc` features.

Those features are re-exposed from the `tonic` crate, please refer to its documentation for more info.
