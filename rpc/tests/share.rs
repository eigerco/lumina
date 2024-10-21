#![cfg(not(target_arch = "wasm32"))]

use celestia_rpc::prelude::*;
use celestia_types::nmt::{Namespace, NamespacedSha2Hasher};
use celestia_types::{Blob, Share};

pub mod utils;

use crate::utils::client::{blob_submit, new_test_client, AuthLevel};
use crate::utils::{random_bytes, random_ns, random_ns_range};

#[tokio::test]
async fn get_share() {
    let client = new_test_client(AuthLevel::Write).await.unwrap();
    let header = client.header_network_head().await.unwrap();
    let square_width = header.dah.square_width() as u64;

    for row in 0..square_width {
        for col in 0..square_width {
            let share = client.share_get_share(&header, row, col).await.unwrap();
            let is_parity = row >= square_width / 2 || col >= square_width / 2;

            assert_eq!(share.is_parity(), is_parity);
        }
    }
}

#[tokio::test]
async fn get_shares_by_namespace() {
    let client = new_test_client(AuthLevel::Write).await.unwrap();
    let namespace = random_ns();
    let blobs: Vec<_> = (0..rand::random::<usize>() % 4)
        .map(|_| {
            let data = random_bytes(1024);
            Blob::new(namespace, data.clone()).unwrap()
        })
        .collect();

    let submitted_height = blob_submit(&client, &blobs).await.unwrap();

    let header = client.header_get_by_height(submitted_height).await.unwrap();

    let ns_shares = client
        .share_get_shares_by_namespace(&header, namespace)
        .await
        .unwrap();

    let reconstructed =
        Blob::reconstruct_all(ns_shares.rows.iter().flat_map(|row| row.shares.iter())).unwrap();

    assert_eq!(reconstructed, blobs);
}

#[tokio::test]
async fn get_shares_by_namespace_forbidden() {
    let client = new_test_client(AuthLevel::Write).await.unwrap();
    let header = client.header_network_head().await.unwrap();

    // those namespaces are forbidden in celestia-node's implementation
    for ns in [Namespace::TAIL_PADDING, Namespace::PARITY_SHARE] {
        client
            .share_get_shares_by_namespace(&header, ns)
            .await
            .unwrap_err();
    }
}

#[tokio::test]
async fn get_shares_range() {
    let client = new_test_client(AuthLevel::Write).await.unwrap();
    let namespace = random_ns();
    let data = random_bytes(1024);
    let blob = Blob::new(namespace, data.clone()).unwrap();
    let commitment = blob.commitment;

    let submitted_height = blob_submit(&client, &[blob]).await.unwrap();

    let header = client.header_get_by_height(submitted_height).await.unwrap();
    let blob_on_chain = client
        .blob_get(submitted_height, namespace, commitment)
        .await
        .unwrap();
    let index = blob_on_chain.index.unwrap();
    let shares = blob_on_chain.to_shares().unwrap();

    let shares_range = client
        .share_get_range(
            submitted_height,
            index as usize,
            index as usize + shares.len(),
        )
        .await
        .unwrap();

    shares_range.proof.verify(header.dah.hash()).unwrap();

    for ((share, received), proven) in shares
        .into_iter()
        .zip(shares_range.shares.into_iter())
        .zip(shares_range.proof.shares().iter())
    {
        assert_eq!(share, Share::try_from(received).unwrap());
        assert_eq!(share.as_ref(), proven.as_ref());
    }
}

#[tokio::test]
async fn get_shares_range_not_existing() {
    let client = new_test_client(AuthLevel::Write).await.unwrap();
    let header = client.header_network_head().await.unwrap();
    let shares_in_block = header.dah.square_width().pow(2);

    client
        .share_get_range(
            header.height().value(),
            shares_in_block as usize - 2,
            shares_in_block as usize + 2,
        )
        .await
        .unwrap_err();
}

#[tokio::test]
async fn get_shares_by_namespace_wrong_ns() {
    let client = new_test_client(AuthLevel::Write).await.unwrap();
    let namespace = random_ns();
    let data = random_bytes(1024);
    let blob = Blob::new(namespace, data.clone()).unwrap();

    let submitted_height = blob_submit(&client, &[blob]).await.unwrap();

    let header = client.header_get_by_height(submitted_height).await.unwrap();

    let root_hash = header.dah.row_root(0).unwrap();
    let min_ns = root_hash.min_namespace().into();
    let max_ns = root_hash.max_namespace().into();

    // When we try to get shares for the unknown namespace then
    // if there exists a row where row_root.min_namespace() < namespace < row_root.max_namespace()
    // then we will get an absence proof for that row
    // The rows where namespace falls outside of the ns range of the row_root
    // are not included in the response.
    // As the block has just a single blob, we can only care about the first row.

    // check the case where we receive absence proof
    let random_ns = random_ns_range(min_ns, max_ns);
    let ns_shares = client
        .share_get_shares_by_namespace(&header, random_ns)
        .await
        .unwrap();
    assert_eq!(ns_shares.rows.len(), 1);
    assert!(ns_shares.rows[0].shares.is_empty());

    let proof = &ns_shares.rows[0].proof;
    assert!(proof.is_of_absence());

    let no_leaves: &[&[u8]] = &[];

    proof
        .verify_complete_namespace(&root_hash, no_leaves, random_ns.into())
        .unwrap();
}

#[tokio::test]
async fn get_shares_by_namespace_wrong_ns_out_of_range() {
    let client = new_test_client(AuthLevel::Write).await.unwrap();
    let namespace = random_ns();
    let data = random_bytes(1024);
    let blob = Blob::new(namespace, data.clone()).unwrap();

    let submitted_height = blob_submit(&client, &[blob]).await.unwrap();

    let header = client.header_get_by_height(submitted_height).await.unwrap();

    let root_hash = header.dah.row_root(0).unwrap();
    let min_ns = root_hash.min_namespace().into();

    // check the case where namespace is outside of the root hash range
    let zero = Namespace::const_v0([0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    let random_ns = random_ns_range(zero, min_ns);
    let ns_shares = client
        .share_get_shares_by_namespace(&header, random_ns)
        .await
        .unwrap();

    assert!(ns_shares.rows.is_empty());
    assert!(!root_hash.contains::<NamespacedSha2Hasher>(random_ns.into()));
}

#[tokio::test]
async fn get_shares_by_namespace_wrong_roots() {
    let client = new_test_client(AuthLevel::Write).await.unwrap();
    let namespace = random_ns();
    let data = random_bytes(1024);
    let blob = Blob::new(namespace, data.clone()).unwrap();

    blob_submit(&client, &[blob]).await.unwrap();

    let genesis = client.header_get_by_height(1).await.unwrap();

    let ns_shares = client
        .share_get_shares_by_namespace(&genesis, namespace)
        .await
        .unwrap();

    assert!(ns_shares.rows.is_empty());
}

#[tokio::test]
async fn get_eds() {
    let client = new_test_client(AuthLevel::Write).await.unwrap();
    let namespace = random_ns();
    let data = vec![1, 2, 3, 4];
    let blob = Blob::new(namespace, data.clone()).unwrap();

    let submitted_height = blob_submit(&client, &[blob]).await.unwrap();

    let header = client.header_get_by_height(submitted_height).await.unwrap();
    let eds = client.share_get_eds(&header).await.unwrap();

    for i in 0..header.dah.square_width() {
        let row_root = eds.row_nmt(i).unwrap().root();
        assert_eq!(row_root, header.dah.row_root(i).unwrap());

        let column_root = eds.column_nmt(i).unwrap().root();
        assert_eq!(column_root, header.dah.column_root(i).unwrap());
    }
}
