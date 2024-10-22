use std::{collections::HashSet, time::Duration};

use lumina_node::events::NodeEvent;
use tokio::time::{sleep, timeout};

use crate::utils::new_connected_node;

mod utils;

#[tokio::test]
async fn sampling_forward() {
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
        let new_head = timeout(Duration::from_millis(1500), get_new_head)
            .await
            .unwrap();

        // wait for height to be sampled
        let wait_height_sampled = async {
            loop {
                let ev = events.recv().await.unwrap();
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

#[tokio::test]
async fn sampling_backward() {
    let (node, mut events) = new_connected_node().await;

    sleep(Duration::from_millis(300)).await;

    let current_head = node.get_local_head_header().await.unwrap().height().value();
    let mut headers_to_sample: HashSet<_> =
        (current_head.saturating_sub(50)..current_head).collect();

    // wait for all heights to be sampled
    timeout(Duration::from_secs(10), async {
        loop {
            let ev = events.recv().await.unwrap();
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
