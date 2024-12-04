# Update

To update protobuf definitions run:

```bash
./tools/update-proto-vendor.sh
```


# How `tendermint-celestia-mods` work and how to maintain it

This is a walk-through on how we mix Celestia's Tendermint modifications to our crates,
without forking `tendermint-rs`. At the time of writing I used the following repositories:

* [celestia-core](https://github.com/celestiaorg/celestia-core) checked-out on `v1.44.0-tm-v0.34.35`
* [tendermint-rs](https://github.com/informalsystems/tendermint-rs) checked-out on `v0.40.0`
* [cometbft](https://github.com/cometbft/cometbft) checked-out on `v0.34.35`

### How to find which protos `tendermint-rs` is using

Open `tendermint-rs/tools/proto-compiler/src/constants.rs` and in `TENDERMINT_VERSIONS`
get the `commitish` field of the 0.34 version.

### Find Celestia's modifications

Run:

```
diff -x '*.go' -urN cometbft/proto/tendermint celestia-core/proto/tendermint
```

The differences are:

* `abci/types.proto`
	* `tendermint.abci.Request` modified
	* `tendermint.abci.RequestPrepareProposal` added
	* `tendermint.abci.RequestProcessProposal` added
	* `tendermint.abci.Response` modified
	* `tendermint.abci.ResponsePrepareProposal` added
	* `tendermint.abci.ResponseProcessProposal` added
	* `tendermint.abci.ABCIApplication` service modified
	* `tendermint.abci.RequestOfferSnapshot` modified
	* `tendermint.abci.TimeoutsInfo` added
	* `tendermint.abci.ResponseInitChain` modified
	*  `tendermint.abci.ResponseInfo` added
	*  `tendermint.abci.ResponseEncBlock` added
* `mempool/types.proto`
	* `tendermint.mempool.SeenTx` added
	* `tendermint.mempool.WantTx` added
	* `tendermint.mempool.Message` modified
* `state/types.proto`
	* `tendermint.state.State` modified
* `store/types.proto`
	* `tendermint.store.TxInfo` added
* `types/types.proto`
	* `tendermint.types.Data` modified
	* `tendermint.types.Blob` added
	* `tendermint.types.IndexWrapper` added
	* `tendermint.types.BlobTx` added
	* `tendermint.types.ShareProof` added
	* `tendermint.types.RowProof` added
	* `tendermint.types.NMTProof` added


### Vendoring approaches

Types of the `tendermint` crate are tightly integrated with types of the `tendermint-proto`
crate, so we've had two choices:

1. Vendor the whole `celestia-core/proto/tendermint` and write type conversion for
   all the types of `tendermint-proto` crate and even wrap all the types of `tendermint` crate.
2. Vendor only the differences and wrap only the affected types of `tendermint` crate.

We decided to choose the latter since it requires much less boilerplate code.

### How to vendor only the differences

> [!NOTE]
> You can find the end result of the following steps [here](https://github.com/eigerco/lumina/tree/82c51f6ac88fd3662a0f91a0cf19a717986e3470/proto/vendor/tendermint-celestia-mods).

**Step 1:**

Copy all modified/added Protobuf messages at `tendermint-celestia-mods` directory
and keep them in same filenames/paths. For example `Data` message should be in
`tendermint-celestia-mods/types/types.proto`.

We also exclude types that where migrated to other Protobuf packages:

* `tendermint.types.Blob` replaced by `proto.blob.v1.BlobProto` ([link](https://github.com/eigerco/lumina/blob/82c51f6ac88fd3662a0f91a0cf19a717986e3470/proto/vendor/go-square/blob/v1/blob.proto#L10-L18)).
* `tendermint.types.BlobTx` replaced by `proto.blob.v1.BlobTx` ([link](https://github.com/eigerco/lumina/blob/82c51f6ac88fd3662a0f91a0cf19a717986e3470/proto/vendor/go-square/blob/v1/blob.proto#L23-L27)).
* `tendermint.types.IndexWrapper` replaced by `proto.blob.v1.IndexWrapper` ([link](https://github.com/eigerco/lumina/blob/82c51f6ac88fd3662a0f91a0cf19a717986e3470/proto/vendor/go-square/blob/v1/blob.proto#L31-L35)).
* `tendermint.types.ShareProof` replaced by `celestia.core.v1.proof.ShareProof` ([link](https://github.com/eigerco/lumina/blob/82c51f6ac88fd3662a0f91a0cf19a717986e3470/proto/vendor/celestia/core/v1/proof/proof.proto#L8-L14)).
* `tendermint.types.RowProof` replaced by `celestia.core.v1.proof.RowProof` ([link](https://github.com/eigerco/lumina/blob/82c51f6ac88fd3662a0f91a0cf19a717986e3470/proto/vendor/celestia/core/v1/proof/proof.proto#L18-L24)).
* `tendermint.types.NMTProof` replaced by `celestia.core.v1.proof.NMTProof` ([link](https://github.com/eigerco/lumina/blob/82c51f6ac88fd3662a0f91a0cf19a717986e3470/proto/vendor/celestia/core/v1/proof/proof.proto#L30-L46)).

**Step 2:**

Rename the `package` of all `.proto` file from `tendermint` to `tendermint_celestia_mods`.
For example in `tendermint-celestia-mods/types/types.proto` the line `package tendermint.types`
must become `package tendermint_celestia_mods.types` ([link](https://github.com/eigerco/lumina/blob/82c51f6ac88fd3662a0f91a0cf19a717986e3470/proto/vendor/tendermint-celestia-mods/types/types.proto#L2)).

**Step 3:**

Whenever you find `import "tendermint/path/to/file.proto"` and the `path/to/file.proto` exists
in `tendermint-celestia-mods` then add `import "tendermint-celestia-mods/path/to/file.proto"`.
For example in `abci/types.proto` we added `import "tendermint-celestia-mods/types/types.proto"`
([link](https://github.com/eigerco/lumina/blob/82c51f6ac88fd3662a0f91a0cf19a717986e3470/proto/vendor/tendermint-celestia-mods/abci/types.proto#L9)).

**Step 4:**

On any fields that their message type was not added/modified by us, we need to change their type
to the `tendermint` one. For example in `abci/types.proto` we changed `RequestEcho` types to
`tendermint.abci.RequestEcho`. It was also needed to add `import "tendermint/abci/types.proto"`
([link 1](https://github.com/eigerco/lumina/blob/82c51f6ac88fd3662a0f91a0cf19a717986e3470/proto/vendor/tendermint-celestia-mods/abci/types.proto#L21),
[link 2](https://github.com/eigerco/lumina/blob/82c51f6ac88fd3662a0f91a0cf19a717986e3470/proto/vendor/tendermint-celestia-mods/abci/types.proto#L148)).

If the message type of the field is redefined in `tendermint-celestia-mods` then the one from the
modifications should be used
([link](https://github.com/oblique/lumina/blob/82c51f6ac88fd3662a0f91a0cf19a717986e3470/proto/vendor/tendermint-celestia-mods/abci/types.proto#L70-L73)).
Keep in mind that if the type exists in the same file, then full path is not needed.

**Step 5:**

Vendor any message type that contains a modified message type. The best example here is the modified `Data`:

* We need to vendor `tendermint.types.Block` in `tendermint-celestia-mods` because it uses `Data` ([link](https://github.com/eigerco/lumina/blob/82c51f6ac88fd3662a0f91a0cf19a717986e3470/proto/vendor/tendermint-celestia-mods/types/block.proto#L9-L14)).
* Then vendor `tendermint.blockchain.BlockResponse` because it uses `tendermint.types.Block` ([link](https://github.com/eigerco/lumina/blob/82c51f6ac88fd3662a0f91a0cf19a717986e3470/proto/vendor/tendermint-celestia-mods/blockchain/types.proto#L9-L11)). Notice that `BlockResponse.block` now has a type of `endermint_celestia_mods.types.Block`.
* Then vendor `tendermint.blockchain.Message` because it uses `tendermint.blockchain.BlockResponse` ([link](https://github.com/eigerco/lumina/blob/82c51f6ac88fd3662a0f91a0cf19a717986e3470/proto/vendor/tendermint-celestia-mods/blockchain/types.proto#L13-L21)).

**Step 6:**

Modify `build.rs`:

* Add the new `.proto` files in `PROTO_FILES` ([link](https://github.com/eigerco/lumina/blob/82c51f6ac88fd3662a0f91a0cf19a717986e3470/proto/build.rs#L136-L142)).
* Adjust `CUSTOM_TYPE_ATTRIBUTES` ([link](https://github.com/eigerco/lumina/blob/82c51f6ac88fd3662a0f91a0cf19a717986e3470/proto/build.rs#L58-L62)) and `CUSTOM_FIELD_ATTRIBUTES` ([link](https://github.com/eigerco/lumina/blob/82c51f6ac88fd3662a0f91a0cf19a717986e3470/proto/build.rs#L91-L101)).
* Map `.tendermint` to `::tendermint_proto::v0_34` via `EXTERN_PATHS` ([link](https://github.com/eigerco/lumina/blob/82c51f6ac88fd3662a0f91a0cf19a717986e3470/proto/build.rs#L109)). This will also inform `prost` that it doesn't need to generate code for `tendermint` directory.
* Add `.bytes([".tendermint_celestia_mods.abci"])` in Prost config. This is needed because [`tendermint-rs` does it](https://github.com/informalsystems/tendermint-rs/blob/2f94e7f5346a094ec98e9019d0181815ccede7f1/tools/proto-compiler/src/main.rs#L89-L90) and we want to keep the same type.
* We [implemented a way](https://github.com/eigerco/lumina/blob/82c51f6ac88fd3662a0f91a0cf19a717986e3470/proto/build.rs#L224-L263) do automatically replace any `.tendermint` types with `.tendermint-celestia-mods` using [`extern_path`](https://docs.rs/prost-build/0.13.3/prost_build/struct.Config.html#method.extern_path). This will make Protobuf definitions (e.g. `cosmos`) to use `tendermint-celestia-mods` types.

### How to integrate modifications

Some rules of thumb:

1. If a type exists in `tendermint-proto` and `celestia-tendermint-mods`, always choose
   the latter. This includes GRPC stubs too.
2. If the type from point 1 has a higher level implementation in `tendermint` crate, then
   reimplement that in `celestia-types` and integrate it with the one from `celestia-tendermint-mods`. 

The best example for point 2 is [`tendermint::block::Block`](https://github.com/informalsystems/tendermint-rs/blob/2f94e7f5346a094ec98e9019d0181815ccede7f1/tendermint/src/block.rs#L36-L51), which we reimplement as [`celestia_types::block::Block`](https://github.com/eigerco/lumina/blob/82c51f6ac88fd3662a0f91a0cf19a717986e3470/types/src/block.rs#L30-L44), and then used in [`celestia_grpc::GrpcClient`](https://github.com/eigerco/lumina/blob/82c51f6ac88fd3662a0f91a0cf19a717986e3470/grpc/src/client.rs#L53-L55). The difference in them is their `data` field.
