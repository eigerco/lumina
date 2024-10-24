use std::{collections::HashSet, time::Duration};

use celestia_rpc::{BlobClient, ShareClient};
use celestia_types::{nmt::Namespace, Blob, TxConfig};
use lumina_node::events::NodeEvent;
use rand::RngCore;
use tokio::time::{sleep, timeout};

use crate::utils::{bridge_client, new_connected_node};

mod utils;

#[tokio::test]
async fn sampling_forward() {
    let (node, mut events) = new_connected_node().await;

    // create new events sub to ignore all previous events
    // let mut events = node.event_subscriber();

    for _ in 0..5 {
        // wait for new block
        let get_new_head = async {
            loop {
                let ev = events.recv().await.unwrap();
                tracing::info!("{}", ev.event);
                let NodeEvent::AddedHeaderFromHeaderSub { height, .. } = ev.event else {
                    continue;
                };
                break height;
            }
        };
        let new_head = timeout(Duration::from_millis(1500), get_new_head)
            .await
            .unwrap();

        // wait for height to be sampled
        let wait_height_sampled = async {
            loop {
                let ev = events.recv().await.unwrap();
                tracing::info!("{}", ev.event);
                let NodeEvent::SamplingFinished {
                    height, accepted, ..
                } = ev.event
                else {
                    continue;
                };

                if height == new_head {
                    assert!(accepted);
                    break;
                }
            }
        };
        timeout(Duration::from_millis(500), wait_height_sampled)
            .await
            .unwrap();
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn sampling_backward() {
    let (node, mut events) = new_connected_node().await;

    let current_head = node.get_local_head_header().await.unwrap().height().value();

    // wait for some past headers to be synchronized
    let new_batch_synced = async {
        loop {
            let ev = events.recv().await.unwrap();
            tracing::info!("{}", ev.event);
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
    let mut headers_to_sample: HashSet<_> = (from_height..to_height).rev().take(50).collect();

    // wait for all heights to be sampled
    timeout(Duration::from_secs(10), async {
        loop {
            let ev = events.recv().await.unwrap();
            tracing::info!("{}", ev.event);
            let NodeEvent::SamplingFinished {
                height, accepted, ..
            } = ev.event
            else {
                continue;
            };

            assert!(accepted);
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
async fn shwap_get() {
    let (node, _) = new_connected_node().await;
    let client = bridge_client().await;

    let ns = Namespace::const_v0(rand::random());
    let blob_len = rand::random::<usize>() % 4096 + 1;
    let blob = Blob::new(ns, random_bytes(blob_len)).unwrap();

    let height = client
        .blob_submit(&[blob], TxConfig::default())
        .await
        .unwrap();

    //let expected = client.share_get_share(0, 0).await.unwrap();
    node.request_sample(0, 0, height, Some(Duration::from_millis(500)))
        .await
        .unwrap();
    node.request_row(0, height, Some(Duration::from_millis(500)))
        .await
        .unwrap();
    node.request_row_namespace_data(ns, 0, height, Some(Duration::from_millis(500)))
        .await
        .unwrap();
}

fn random_bytes(len: usize) -> Vec<u8> {
    let mut bytes = vec![0u8; len];
    rand::thread_rng().fill_bytes(&mut bytes);
    bytes
}
