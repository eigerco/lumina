#![cfg(not(target_arch = "wasm32"))]

use std::{collections::HashSet, time::Duration};

use celestia_rpc::ShareClient;
use celestia_types::{
    nmt::{Namespace, NamespacedSha2Hasher},
    AppVersion, Blob,
};
use lumina_node::{events::NodeEvent, node::P2pError, NodeError};
use rand::RngCore;
use tokio::time::timeout;

use crate::utils::{blob_submit, bridge_client, new_connected_node};

mod utils;

#[tokio::test]
async fn shwap_sampling_forward() {
    let (node, _) = new_connected_node().await;

    // create new events sub to ignore all previous events
    let mut events = node.event_subscriber();

    for _ in 0..5 {
        // wait for new block
        let get_new_head = async {
            loop {
                let ev = events.recv().await.unwrap();
                let NodeEvent::AddedHeaderFromHeaderSub { height, .. } = ev.event else {
                    continue;
                };
                break height;
            }
        };
        // timeout is double of the block time on CI
        let new_head = timeout(Duration::from_secs(9), get_new_head).await.unwrap();

        // wait for height to be sampled
        let wait_height_sampled = async {
            loop {
                let ev = events.recv().await.unwrap();
                let NodeEvent::SamplingResult {
                    height, timed_out, ..
                } = ev.event
                else {
                    continue;
                };

                if height == new_head {
                    assert!(!timed_out);
                    break;
                }
            }
        };
        timeout(Duration::from_secs(1), wait_height_sampled)
            .await
            .unwrap();
    }
}

#[tokio::test]
async fn shwap_sampling_backward() {
    let (node, mut events) = new_connected_node().await;

    let current_head = node.get_local_head_header().await.unwrap().height().value();

    // wait for some past headers to be synchronized
    let new_batch_synced = async {
        loop {
            let ev = events.recv().await.unwrap();
            let NodeEvent::FetchingHeadersFinished {
                from_height,
                to_height,
                ..
            } = ev.event
            else {
                continue;
            };
            if to_height < current_head {
                break (from_height, to_height);
            }
        }
    };
    let (from_height, to_height) = timeout(Duration::from_secs(4), new_batch_synced)
        .await
        .unwrap();

    // take just first N headers because batch size can be big
    let mut headers_to_sample: HashSet<_> = (from_height..to_height).rev().take(10).collect();

    // wait for all heights to be sampled
    timeout(Duration::from_secs(10), async {
        loop {
            let ev = events.recv().await.unwrap();
            let NodeEvent::SamplingResult {
                height, timed_out, ..
            } = ev.event
            else {
                continue;
            };

            assert!(!timed_out);
            headers_to_sample.remove(&height);

            if headers_to_sample.is_empty() {
                break;
            }
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn shwap_request_sample() {
    let (node, _) = new_connected_node().await;
    let client = bridge_client().await;

    let ns = Namespace::const_v0(rand::random());
    let blob_len = rand::random::<usize>() % 4096 + 1;
    let blob = Blob::new(ns, random_bytes(blob_len), AppVersion::V2).unwrap();

    let height = blob_submit(&client, &[blob]).await;
    let header = node.get_header_by_height(height).await.unwrap();
    let square_width = header.dah.square_width();

    // check existing sample
    let expected = client.share_get_share(&header, 0, 0).await.unwrap();
    let sample = node
        .request_sample(0, 0, height, Some(Duration::from_millis(500)))
        .await
        .unwrap();
    assert_eq!(expected, sample.share);

    // check nonexisting sample
    let err = node
        .request_sample(
            square_width + 1,
            square_width + 1,
            height,
            Some(Duration::from_millis(500)),
        )
        .await
        .unwrap_err();
    assert!(matches!(err, NodeError::P2p(P2pError::BitswapQueryTimeout)));
}

#[tokio::test]
async fn shwap_request_row() {
    let (node, _) = new_connected_node().await;
    let client = bridge_client().await;

    let ns = Namespace::const_v0(rand::random());
    let blob_len = rand::random::<usize>() % 4096 + 1;
    let blob = Blob::new(ns, random_bytes(blob_len), AppVersion::V2).unwrap();

    let height = blob_submit(&client, &[blob]).await;
    let header = node.get_header_by_height(height).await.unwrap();
    let eds = client.share_get_eds(&header).await.unwrap();
    let square_width = header.dah.square_width();

    // check existing row
    let row = node
        .request_row(0, height, Some(Duration::from_secs(1)))
        .await
        .unwrap();
    assert_eq!(eds.row(0).unwrap(), row.shares);

    // check nonexisting row
    let err = node
        .request_row(square_width + 1, height, Some(Duration::from_secs(1)))
        .await
        .unwrap_err();
    assert!(matches!(err, NodeError::P2p(P2pError::BitswapQueryTimeout)));
}

#[tokio::test]
async fn shwap_request_row_namespace_data() {
    let (node, _) = new_connected_node().await;
    let client = bridge_client().await;

    let ns = Namespace::const_v0(rand::random());
    let blob_len = rand::random::<usize>() % 4096 + 1;
    let blob = Blob::new(ns, random_bytes(blob_len), AppVersion::V2).unwrap();

    let height = blob_submit(&client, &[blob]).await;
    let header = node.get_header_by_height(height).await.unwrap();
    let eds = client.share_get_eds(&header).await.unwrap();
    let square_width = header.dah.square_width();

    // check existing row namespace data
    let rows_with_ns: Vec<_> = header
        .dah
        .row_roots()
        .iter()
        .enumerate()
        .filter_map(|(n, hash)| {
            hash.contains::<NamespacedSha2Hasher>(*ns)
                .then_some(n as u16)
        })
        .collect();
    let eds_ns_data = eds.get_namespace_data(ns, &header.dah, height).unwrap();

    for (n, &row) in rows_with_ns.iter().enumerate() {
        let row_ns_data = node
            .request_row_namespace_data(ns, row, height, Some(Duration::from_secs(1)))
            .await
            .unwrap();
        assert_eq!(eds_ns_data[n].1, row_ns_data);
    }

    // check nonexisting row row namespace data
    let err = node
        .request_row_namespace_data(ns, square_width + 1, height, Some(Duration::from_secs(1)))
        .await
        .unwrap_err();
    assert!(matches!(err, NodeError::P2p(P2pError::BitswapQueryTimeout)));

    // check nonexisting namespace row namespace data
    // for namespace that row actually contains
    // PFB (0x04) < 0x05 < Primary ns padding (0x255)
    let unknown_ns = Namespace::const_v0([0, 0, 0, 0, 0, 0, 0, 0, 0, 5]);
    let row = node
        .request_row_namespace_data(unknown_ns, 0, height, Some(Duration::from_secs(1)))
        .await
        .unwrap();
    assert!(row.shares.is_empty());

    // check nonexisting namespace row namespace data
    // for namespace that row doesn't contain
    let unknown_ns = Namespace::TAIL_PADDING;
    let err = node
        .request_row_namespace_data(unknown_ns, 0, height, Some(Duration::from_secs(1)))
        .await
        .unwrap_err();
    assert!(matches!(err, NodeError::P2p(P2pError::BitswapQueryTimeout)));
}

#[tokio::test]
async fn shwap_request_all_blobs() {
    let (node, _) = new_connected_node().await;
    let client = bridge_client().await;

    let ns = Namespace::const_v0(rand::random());
    let blobs: Vec<_> = (0..5)
        .map(|_| {
            let blob_len = rand::random::<usize>() % 4096 + 1;
            Blob::new(ns, random_bytes(blob_len), AppVersion::V2).unwrap()
        })
        .collect();

    let height = blob_submit(&client, &blobs).await;

    // check existing namespace
    let received = node
        .request_all_blobs(ns, height, Some(Duration::from_secs(2)))
        .await
        .unwrap();

    assert_eq!(blobs, received);

    // check nonexisting namespace
    let ns = Namespace::const_v0(rand::random());
    let received = node
        .request_all_blobs(ns, height, Some(Duration::from_secs(2)))
        .await
        .unwrap();

    assert!(received.is_empty());
}

fn random_bytes(len: usize) -> Vec<u8> {
    let mut bytes = vec![0u8; len];
    rand::thread_rng().fill_bytes(&mut bytes);
    bytes
}
