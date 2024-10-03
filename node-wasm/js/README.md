
<a name="readmemd"></a>

**lumina-node-wasm** • [**Docs**](#globalsmd)

***

# Lumina node wasm

A compatibility layer for the [`Lumina`](https://github.com/eigerco/lumina) node to
work within a browser environment and be operable with javascript.

# Example
Starting lumina inside a dedicated worker

```javascript
import { spawnNode, NodeConfig, Network } from "lumina-node";

const node = await spawnNode();
const mainnetConfig = NodeConfig.default(Network.Mainnet);

await node.start(mainnetConfig);

await node.waitConnected();
await node.requestHeadHeader();
```

## Manual setup

Note that `spawnNode` implicitly calls wasm initialisation code. If you want to set things up manually, make sure to call the default export before using any of the wasm functionality.

```javascript
import init, { NodeConfig, Network } from "lumina-node";

await init();
const config = NodeConfig.default(Network.Mainnet);

// client and worker accept any object with MessagePort like interface e.g. Worker
const channel = new MessageChannel();
const worker = new NodeWorker(channel.port1);

// note that this runs lumina in the current context (and doesn't create a new web-worker). Promise created with `.run()` never completes.
const worker_promise = worker.run();

// client port can be used locally or transferred like any plain MessagePort
const client = await new NodeClient(channel.port2);
await client.waitConnected();
await client.requestHeadHeader();
```

## Rust API

For comprehensive and fully typed interface documentation, see [lumina-node](https://docs.rs/lumina-node/latest/lumina_node/) and [celestia-types](https://docs.rs/celestia-types/latest/celestia_types/) documentation on docs.rs. You can see there the exact structure of more complex types, such as [`ExtendedHeader`](https://docs.rs/celestia-types/latest/celestia_types/struct.ExtendedHeader.html). JavaScript API's goal is to provide similar interface to Rust when possible, e.g. `NodeClient` mirrors [`Node`](https://docs.rs/lumina-node/latest/lumina_node/node/struct.Node.html).

# Classes


<a name="classesblockrangemd"></a>

[**lumina-node-wasm**](#readmemd) • **Docs**

***

[lumina-node-wasm](#globalsmd) / BlockRange

## Class: BlockRange

A range of blocks between `start` and `end` height, inclusive

### Constructors

#### new BlockRange()

> **new BlockRange**(): [`BlockRange`](#classesblockrangemd)

##### Returns

[`BlockRange`](#classesblockrangemd)

### Properties

#### end

> **end**: `bigint`

Last block height in range

##### Defined in

lumina\_node\_wasm.d.ts:44

***

#### start

> **start**: `bigint`

First block height in range

##### Defined in

lumina\_node\_wasm.d.ts:48

### Methods

#### free()

> **free**(): `void`

##### Returns

`void`

##### Defined in

lumina\_node\_wasm.d.ts:40

***

#### toJSON()

> **toJSON**(): `Object`

* Return copy of self without private attributes.

##### Returns

`Object`

##### Defined in

lumina\_node\_wasm.d.ts:35

***

#### toString()

> **toString**(): `string`

Return stringified version of self.

##### Returns

`string`

##### Defined in

lumina\_node\_wasm.d.ts:39


<a name="classesconnectioncounterssnapshotmd"></a>

[**lumina-node-wasm**](#readmemd) • **Docs**

***

[lumina-node-wasm](#globalsmd) / ConnectionCountersSnapshot

## Class: ConnectionCountersSnapshot

### Constructors

#### new ConnectionCountersSnapshot()

> **new ConnectionCountersSnapshot**(): [`ConnectionCountersSnapshot`](#classesconnectioncounterssnapshotmd)

##### Returns

[`ConnectionCountersSnapshot`](#classesconnectioncounterssnapshotmd)

### Properties

#### num\_connections

> **num\_connections**: `number`

The total number of connections, both pending and established.

##### Defined in

lumina\_node\_wasm.d.ts:65

***

#### num\_established

> **num\_established**: `number`

The number of outgoing connections being established.

##### Defined in

lumina\_node\_wasm.d.ts:69

***

#### num\_established\_incoming

> **num\_established\_incoming**: `number`

The number of established incoming connections.

##### Defined in

lumina\_node\_wasm.d.ts:73

***

#### num\_established\_outgoing

> **num\_established\_outgoing**: `number`

The number of established outgoing connections.

##### Defined in

lumina\_node\_wasm.d.ts:77

***

#### num\_pending

> **num\_pending**: `number`

The total number of pending connections, both incoming and outgoing.

##### Defined in

lumina\_node\_wasm.d.ts:81

***

#### num\_pending\_incoming

> **num\_pending\_incoming**: `number`

The total number of pending connections, both incoming and outgoing.

##### Defined in

lumina\_node\_wasm.d.ts:85

***

#### num\_pending\_outgoing

> **num\_pending\_outgoing**: `number`

The number of outgoing connections being established.

##### Defined in

lumina\_node\_wasm.d.ts:89

### Methods

#### free()

> **free**(): `void`

##### Returns

`void`

##### Defined in

lumina\_node\_wasm.d.ts:61

***

#### toJSON()

> **toJSON**(): `Object`

* Return copy of self without private attributes.

##### Returns

`Object`

##### Defined in

lumina\_node\_wasm.d.ts:56

***

#### toString()

> **toString**(): `string`

Return stringified version of self.

##### Returns

`string`

##### Defined in

lumina\_node\_wasm.d.ts:60


<a name="classesnetworkinfosnapshotmd"></a>

[**lumina-node-wasm**](#readmemd) • **Docs**

***

[lumina-node-wasm](#globalsmd) / NetworkInfoSnapshot

## Class: NetworkInfoSnapshot

Information about the connections

### Constructors

#### new NetworkInfoSnapshot()

> **new NetworkInfoSnapshot**(): [`NetworkInfoSnapshot`](#classesnetworkinfosnapshotmd)

##### Returns

[`NetworkInfoSnapshot`](#classesnetworkinfosnapshotmd)

### Properties

#### connection\_counters

> **connection\_counters**: [`ConnectionCountersSnapshot`](#classesconnectioncounterssnapshotmd)

Gets counters for ongoing network connections.

##### Defined in

lumina\_node\_wasm.d.ts:107

***

#### num\_peers

> **num\_peers**: `number`

The number of connected peers, i.e. peers with whom at least one established connection exists.

##### Defined in

lumina\_node\_wasm.d.ts:111

### Methods

#### free()

> **free**(): `void`

##### Returns

`void`

##### Defined in

lumina\_node\_wasm.d.ts:103

***

#### toJSON()

> **toJSON**(): `Object`

* Return copy of self without private attributes.

##### Returns

`Object`

##### Defined in

lumina\_node\_wasm.d.ts:98

***

#### toString()

> **toString**(): `string`

Return stringified version of self.

##### Returns

`string`

##### Defined in

lumina\_node\_wasm.d.ts:102


<a name="classesnodeclientmd"></a>

[**lumina-node-wasm**](#readmemd) • **Docs**

***

[lumina-node-wasm](#globalsmd) / NodeClient

## Class: NodeClient

`NodeClient` is responsible for steering [`NodeWorker`] by sending it commands and receiving
responses over the provided port.

[`NodeWorker`]: crate::worker::NodeWorker

### Constructors

#### new NodeClient()

> **new NodeClient**(`port`): [`NodeClient`](#classesnodeclientmd)

Create a new connection to a Lumina node running in [`NodeWorker`]. Provided `port` is
expected to have `MessagePort`-like interface for sending and receiving messages.

##### Parameters

• **port**: `any`

##### Returns

[`NodeClient`](#classesnodeclientmd)

##### Defined in

lumina\_node\_wasm.d.ts:126

### Methods

#### addConnectionToWorker()

> **addConnectionToWorker**(`port`): `Promise`\<`void`\>

Establish a new connection to the existing worker over provided port

##### Parameters

• **port**: `any`

##### Returns

`Promise`\<`void`\>

##### Defined in

lumina\_node\_wasm.d.ts:132

***

#### close()

> **close**(): `Promise`\<`void`\>

Requests SharedWorker running lumina to close. Any events received afterwards wont
be processed and new NodeClient needs to be created to restart a node.

##### Returns

`Promise`\<`void`\>

##### Defined in

lumina\_node\_wasm.d.ts:295

***

#### connectedPeers()

> **connectedPeers**(): `Promise`\<`any`[]\>

Get all the peers that node is connected to.

##### Returns

`Promise`\<`any`[]\>

##### Defined in

lumina\_node\_wasm.d.ts:178

***

#### eventsChannel()

> **eventsChannel**(): `Promise`\<`BroadcastChannel`\>

Returns a [`BroadcastChannel`] for events generated by [`Node`].

##### Returns

`Promise`\<`BroadcastChannel`\>

##### Defined in

lumina\_node\_wasm.d.ts:300

***

#### free()

> **free**(): `void`

##### Returns

`void`

##### Defined in

lumina\_node\_wasm.d.ts:120

***

#### getHeaderByHash()

> **getHeaderByHash**(`hash`): `Promise`\<`any`\>

Get a synced header for the block with a given hash.

Returns a javascript object with given structure:
https://docs.rs/celestia-types/latest/celestia_types/struct.ExtendedHeader.html

##### Parameters

• **hash**: `string`

##### Returns

`Promise`\<`any`\>

##### Defined in

lumina\_node\_wasm.d.ts:253

***

#### getHeaderByHeight()

> **getHeaderByHeight**(`height`): `Promise`\<`any`\>

Get a synced header for the block with a given height.

Returns a javascript object with given structure:
https://docs.rs/celestia-types/latest/celestia_types/struct.ExtendedHeader.html

##### Parameters

• **height**: `bigint`

##### Returns

`Promise`\<`any`\>

##### Defined in

lumina\_node\_wasm.d.ts:262

***

#### getHeaders()

> **getHeaders**(`start_height`?, `end_height`?): `Promise`\<`any`[]\>

Get synced headers from the given heights range.

If start of the range is undefined (None), the first returned header will be of height 1.
If end of the range is undefined (None), the last returned header will be the last header in the
store.

## Errors

If range contains a height of a header that is not found in the store.

Returns an array of javascript objects with given structure:
https://docs.rs/celestia-types/latest/celestia_types/struct.ExtendedHeader.html

##### Parameters

• **start\_height?**: `bigint`

• **end\_height?**: `bigint`

##### Returns

`Promise`\<`any`[]\>

##### Defined in

lumina\_node\_wasm.d.ts:280

***

#### getLocalHeadHeader()

> **getLocalHeadHeader**(): `Promise`\<`any`\>

Get the latest locally synced header.

Returns a javascript object with given structure:
https://docs.rs/celestia-types/latest/celestia_types/struct.ExtendedHeader.html

##### Returns

`Promise`\<`any`\>

##### Defined in

lumina\_node\_wasm.d.ts:244

***

#### getNetworkHeadHeader()

> **getNetworkHeadHeader**(): `Promise`\<`any`\>

Get the latest header announced in the network.

Returns a javascript object with given structure:
https://docs.rs/celestia-types/latest/celestia_types/struct.ExtendedHeader.html

##### Returns

`Promise`\<`any`\>

##### Defined in

lumina\_node\_wasm.d.ts:236

***

#### getSamplingMetadata()

> **getSamplingMetadata**(`height`): `Promise`\<`any`\>

Get data sampling metadata of an already sampled height.

Returns a javascript object with given structure:
https://docs.rs/lumina-node/latest/lumina_node/store/struct.SamplingMetadata.html

##### Parameters

• **height**: `bigint`

##### Returns

`Promise`\<`any`\>

##### Defined in

lumina\_node\_wasm.d.ts:289

***

#### isRunning()

> **isRunning**(): `Promise`\<`boolean`\>

Check whether Lumina is currently running

##### Returns

`Promise`\<`boolean`\>

##### Defined in

lumina\_node\_wasm.d.ts:137

***

#### listeners()

> **listeners**(): `Promise`\<`any`[]\>

Get all the multiaddresses on which the node listens.

##### Returns

`Promise`\<`any`[]\>

##### Defined in

lumina\_node\_wasm.d.ts:173

***

#### localPeerId()

> **localPeerId**(): `Promise`\<`string`\>

Get node's local peer ID.

##### Returns

`Promise`\<`string`\>

##### Defined in

lumina\_node\_wasm.d.ts:148

***

#### networkInfo()

> **networkInfo**(): `Promise`\<[`NetworkInfoSnapshot`](#classesnetworkinfosnapshotmd)\>

Get current network info.

##### Returns

`Promise`\<[`NetworkInfoSnapshot`](#classesnetworkinfosnapshotmd)\>

##### Defined in

lumina\_node\_wasm.d.ts:168

***

#### peerTrackerInfo()

> **peerTrackerInfo**(): `Promise`\<[`PeerTrackerInfoSnapshot`](#classespeertrackerinfosnapshotmd)\>

Get current [`PeerTracker`] info.

##### Returns

`Promise`\<[`PeerTrackerInfoSnapshot`](#classespeertrackerinfosnapshotmd)\>

##### Defined in

lumina\_node\_wasm.d.ts:153

***

#### requestHeaderByHash()

> **requestHeaderByHash**(`hash`): `Promise`\<`any`\>

Request a header for the block with a given hash from the network.

Returns a javascript object with given structure:
https://docs.rs/celestia-types/latest/celestia_types/struct.ExtendedHeader.html

##### Parameters

• **hash**: `string`

##### Returns

`Promise`\<`any`\>

##### Defined in

lumina\_node\_wasm.d.ts:202

***

#### requestHeaderByHeight()

> **requestHeaderByHeight**(`height`): `Promise`\<`any`\>

Request a header for the block with a given height from the network.

Returns a javascript object with given structure:
https://docs.rs/celestia-types/latest/celestia_types/struct.ExtendedHeader.html

##### Parameters

• **height**: `bigint`

##### Returns

`Promise`\<`any`\>

##### Defined in

lumina\_node\_wasm.d.ts:211

***

#### requestHeadHeader()

> **requestHeadHeader**(): `Promise`\<`any`\>

Request the head header from the network.

Returns a javascript object with given structure:
https://docs.rs/celestia-types/latest/celestia_types/struct.ExtendedHeader.html

##### Returns

`Promise`\<`any`\>

##### Defined in

lumina\_node\_wasm.d.ts:193

***

#### requestVerifiedHeaders()

> **requestVerifiedHeaders**(`from_header`, `amount`): `Promise`\<`any`[]\>

Request headers in range (from, from + amount] from the network.

The headers will be verified with the `from` header.

Returns an array of javascript objects with given structure:
https://docs.rs/celestia-types/latest/celestia_types/struct.ExtendedHeader.html

##### Parameters

• **from\_header**: `any`

• **amount**: `bigint`

##### Returns

`Promise`\<`any`[]\>

##### Defined in

lumina\_node\_wasm.d.ts:223

***

#### setPeerTrust()

> **setPeerTrust**(`peer_id`, `is_trusted`): `Promise`\<`void`\>

Trust or untrust the peer with a given ID.

##### Parameters

• **peer\_id**: `string`

• **is\_trusted**: `boolean`

##### Returns

`Promise`\<`void`\>

##### Defined in

lumina\_node\_wasm.d.ts:185

***

#### start()

> **start**(`config`): `Promise`\<`void`\>

Start a node with the provided config, if it's not running

##### Parameters

• **config**: [`NodeConfig`](#classesnodeconfigmd)

##### Returns

`Promise`\<`void`\>

##### Defined in

lumina\_node\_wasm.d.ts:143

***

#### syncerInfo()

> **syncerInfo**(): `Promise`\<[`SyncingInfoSnapshot`](#classessyncinginfosnapshotmd)\>

Get current header syncing info.

##### Returns

`Promise`\<[`SyncingInfoSnapshot`](#classessyncinginfosnapshotmd)\>

##### Defined in

lumina\_node\_wasm.d.ts:228

***

#### waitConnected()

> **waitConnected**(): `Promise`\<`void`\>

Wait until the node is connected to at least 1 peer.

##### Returns

`Promise`\<`void`\>

##### Defined in

lumina\_node\_wasm.d.ts:158

***

#### waitConnectedTrusted()

> **waitConnectedTrusted**(): `Promise`\<`void`\>

Wait until the node is connected to at least 1 trusted peer.

##### Returns

`Promise`\<`void`\>

##### Defined in

lumina\_node\_wasm.d.ts:163


<a name="classesnodeconfigmd"></a>

[**lumina-node-wasm**](#readmemd) • **Docs**

***

[lumina-node-wasm](#globalsmd) / NodeConfig

## Class: NodeConfig

Config for the lumina wasm node.

### Constructors

#### new NodeConfig()

> **new NodeConfig**(): [`NodeConfig`](#classesnodeconfigmd)

##### Returns

[`NodeConfig`](#classesnodeconfigmd)

### Properties

#### bootnodes

> **bootnodes**: `string`[]

A list of bootstrap peers to connect to.

##### Defined in

lumina\_node\_wasm.d.ts:324

***

#### network

> **network**: [`Network`](#enumerationsnetworkmd)

A network to connect to.

##### Defined in

lumina\_node\_wasm.d.ts:328

### Methods

#### free()

> **free**(): `void`

##### Returns

`void`

##### Defined in

lumina\_node\_wasm.d.ts:314

***

#### toJSON()

> **toJSON**(): `Object`

* Return copy of self without private attributes.

##### Returns

`Object`

##### Defined in

lumina\_node\_wasm.d.ts:309

***

#### toString()

> **toString**(): `string`

Return stringified version of self.

##### Returns

`string`

##### Defined in

lumina\_node\_wasm.d.ts:313

***

#### default()

> `static` **default**(`network`): [`NodeConfig`](#classesnodeconfigmd)

Get the configuration with default bootnodes for provided network

##### Parameters

• **network**: [`Network`](#enumerationsnetworkmd)

##### Returns

[`NodeConfig`](#classesnodeconfigmd)

##### Defined in

lumina\_node\_wasm.d.ts:320


<a name="classesnodeworkermd"></a>

[**lumina-node-wasm**](#readmemd) • **Docs**

***

[lumina-node-wasm](#globalsmd) / NodeWorker

## Class: NodeWorker

### Constructors

#### new NodeWorker()

> **new NodeWorker**(`port_like_object`): [`NodeWorker`](#classesnodeworkermd)

##### Parameters

• **port\_like\_object**: `any`

##### Returns

[`NodeWorker`](#classesnodeworkermd)

##### Defined in

lumina\_node\_wasm.d.ts:337

### Methods

#### free()

> **free**(): `void`

##### Returns

`void`

##### Defined in

lumina\_node\_wasm.d.ts:333

***

#### run()

> **run**(): `Promise`\<`void`\>

##### Returns

`Promise`\<`void`\>

##### Defined in

lumina\_node\_wasm.d.ts:341


<a name="classespeertrackerinfosnapshotmd"></a>

[**lumina-node-wasm**](#readmemd) • **Docs**

***

[lumina-node-wasm](#globalsmd) / PeerTrackerInfoSnapshot

## Class: PeerTrackerInfoSnapshot

Statistics of the connected peers

### Constructors

#### new PeerTrackerInfoSnapshot()

> **new PeerTrackerInfoSnapshot**(): [`PeerTrackerInfoSnapshot`](#classespeertrackerinfosnapshotmd)

##### Returns

[`PeerTrackerInfoSnapshot`](#classespeertrackerinfosnapshotmd)

### Properties

#### num\_connected\_peers

> **num\_connected\_peers**: `bigint`

Number of the connected peers.

##### Defined in

lumina\_node\_wasm.d.ts:359

***

#### num\_connected\_trusted\_peers

> **num\_connected\_trusted\_peers**: `bigint`

Number of the connected trusted peers.

##### Defined in

lumina\_node\_wasm.d.ts:363

### Methods

#### free()

> **free**(): `void`

##### Returns

`void`

##### Defined in

lumina\_node\_wasm.d.ts:355

***

#### toJSON()

> **toJSON**(): `Object`

* Return copy of self without private attributes.

##### Returns

`Object`

##### Defined in

lumina\_node\_wasm.d.ts:350

***

#### toString()

> **toString**(): `string`

Return stringified version of self.

##### Returns

`string`

##### Defined in

lumina\_node\_wasm.d.ts:354


<a name="classessyncinginfosnapshotmd"></a>

[**lumina-node-wasm**](#readmemd) • **Docs**

***

[lumina-node-wasm](#globalsmd) / SyncingInfoSnapshot

## Class: SyncingInfoSnapshot

Status of the synchronization.

### Constructors

#### new SyncingInfoSnapshot()

> **new SyncingInfoSnapshot**(): [`SyncingInfoSnapshot`](#classessyncinginfosnapshotmd)

##### Returns

[`SyncingInfoSnapshot`](#classessyncinginfosnapshotmd)

### Properties

#### stored\_headers

> **stored\_headers**: [`BlockRange`](#classesblockrangemd)[]

Ranges of headers that are already synchronised

##### Defined in

lumina\_node\_wasm.d.ts:381

***

#### subjective\_head

> **subjective\_head**: `bigint`

Syncing target. The latest height seen in the network that was successfully verified.

##### Defined in

lumina\_node\_wasm.d.ts:385

### Methods

#### free()

> **free**(): `void`

##### Returns

`void`

##### Defined in

lumina\_node\_wasm.d.ts:377

***

#### toJSON()

> **toJSON**(): `Object`

* Return copy of self without private attributes.

##### Returns

`Object`

##### Defined in

lumina\_node\_wasm.d.ts:372

***

#### toString()

> **toString**(): `string`

Return stringified version of self.

##### Returns

`string`

##### Defined in

lumina\_node\_wasm.d.ts:376

# Enumerations


<a name="enumerationsnetworkmd"></a>

[**lumina-node-wasm**](#readmemd) • **Docs**

***

[lumina-node-wasm](#globalsmd) / Network

## Enumeration: Network

Supported Celestia networks.

### Enumeration Members

#### Arabica

> **Arabica**: `1`

Arabica testnet.

##### Defined in

lumina\_node\_wasm.d.ts:18

***

#### Mainnet

> **Mainnet**: `0`

Celestia mainnet.

##### Defined in

lumina\_node\_wasm.d.ts:14

***

#### Mocha

> **Mocha**: `2`

Mocha testnet.

##### Defined in

lumina\_node\_wasm.d.ts:22

***

#### Private

> **Private**: `3`

Private local network.

##### Defined in

lumina\_node\_wasm.d.ts:26

# Functions


<a name="functionssetup_loggingmd"></a>

[**lumina-node-wasm**](#readmemd) • **Docs**

***

[lumina-node-wasm](#globalsmd) / setup\_logging

## Function: setup\_logging()

> **setup\_logging**(): `void`

Set up a logging layer that direct logs to the browser's console.

### Returns

`void`

### Defined in

lumina\_node\_wasm.d.ts:6


<a name="globalsmd"></a>

[**lumina-node-wasm**](#readmemd) • **Docs**

***

# lumina-node-wasm

## Enumerations

- [Network](#enumerationsnetworkmd)

## Classes

- [BlockRange](#classesblockrangemd)
- [ConnectionCountersSnapshot](#classesconnectioncounterssnapshotmd)
- [NetworkInfoSnapshot](#classesnetworkinfosnapshotmd)
- [NodeClient](#classesnodeclientmd)
- [NodeConfig](#classesnodeconfigmd)
- [NodeWorker](#classesnodeworkermd)
- [PeerTrackerInfoSnapshot](#classespeertrackerinfosnapshotmd)
- [SyncingInfoSnapshot](#classessyncinginfosnapshotmd)

## Functions

- [setup\_logging](#functionssetup_loggingmd)
