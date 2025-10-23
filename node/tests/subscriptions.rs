use std::{array::from_ref, time::Duration};

use celestia_types::{Blob, nmt::Namespace};
use futures::stream::StreamExt;
use lumina_utils::time::sleep;

use crate::utils::{blob_submit, bridge_client, new_connected_node};

mod utils;

#[tokio::test]
async fn header_subscription() {
    let (node, _) = new_connected_node().await;

    let mut header_stream = node.header_subscribe().await.unwrap();
    let current_head = node.get_local_head_header().await.unwrap();

    let h0 = header_stream.next().await.unwrap().unwrap();
    // we'll get either current_head, or the next header
    assert!(h0.height().value() - current_head.height().value() <= 1);
    let h1 = header_stream.next().await.unwrap().unwrap();
    assert_eq!(h0.height().value() + 1, h1.height().value());
    let h2 = header_stream.next().await.unwrap().unwrap();
    assert_eq!(h1.height().value() + 1, h2.height().value());
    let h3 = header_stream.next().await.unwrap().unwrap();
    let h3_height = h3.height().value();
    assert_eq!(h2.height().value() + 1, h3_height);
    let tail_header = node.get_header_by_height(h3_height).await.unwrap();
    assert_eq!(h3, tail_header);
}

#[tokio::test]
async fn blob_subscription() {
    let (node, _) = new_connected_node().await;
    let client = bridge_client().await;

    let namespace = Namespace::new_v0(&[200, 199, 198, 197, 196, 195, 194, 193, 192, 191]).unwrap();
    let data = b"bleb0";
    let blob = Blob::new(
        namespace,
        data.to_vec(),
        None,
        celestia_types::AppVersion::V6,
    )
    .unwrap();

    let mut blob_stream = node.namespace_subscribe(namespace).await.unwrap();

    let submitted_at = blob_submit(&client, from_ref(&blob)).await;

    sleep(Duration::from_secs(2)).await;

    let blobs = loop {
        let (h, bs) = blob_stream.next().await.unwrap().unwrap();
        if h == submitted_at {
            break bs;
        }
        assert!(h < submitted_at);
        assert!(bs.is_empty());
    };

    assert_eq!(&blobs, from_ref(&blob));

    let (_h, bs) = blob_stream.next().await.unwrap().unwrap();
    assert_eq!(bs, vec![]);
}
