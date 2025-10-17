use std::array::from_ref;

use celestia_types::{nmt::Namespace, Blob};
use futures::stream::StreamExt;

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

    let (_, bs0) = blob_stream.next().await.unwrap().unwrap();
    assert!(bs0.is_empty());

    let submitted_at = blob_submit(&client, from_ref(&blob)).await;

    loop {
        let (h, bs) = blob_stream.next().await.unwrap().unwrap();
        assert!(bs.is_empty());
        assert!(h < submitted_at);
        if h == submitted_at - 1 {
            break;
        }
    }

    let (h, bs) = blob_stream.next().await.unwrap().unwrap();
    assert_eq!(&bs, &[blob]);
    assert_eq!(h, submitted_at);
}
