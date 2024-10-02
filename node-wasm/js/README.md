<!-- START doctoc generated TOC please keep comment here to allow auto update -->
<!-- DON'T EDIT THIS SECTION, INSTEAD RE-RUN doctoc TO UPDATE -->

- [Lumina node wasm](#lumina-node-wasm)
- [Example](#example)
  - [Manual setup](#manual-setup)
- [Classes](#classes)
  - [Class: BlockRange](#class-blockrange)
    - [Constructors](#constructors)
    - [Properties](#properties)
    - [Methods](#methods)
  - [Class: ConnectionCountersSnapshot](#class-connectioncounterssnapshot)
    - [Constructors](#constructors-1)
    - [Properties](#properties-1)
    - [Methods](#methods-1)
  - [Class: NetworkInfoSnapshot](#class-networkinfosnapshot)
    - [Constructors](#constructors-2)
    - [Properties](#properties-2)
    - [Methods](#methods-2)
  - [Class: NodeClient](#class-nodeclient)
    - [Constructors](#constructors-3)
    - [Methods](#methods-3)
  - [Errors](#errors)
  - [Class: NodeConfig](#class-nodeconfig)
    - [Constructors](#constructors-4)
    - [Properties](#properties-3)
    - [Methods](#methods-4)
  - [Class: NodeWorker](#class-nodeworker)
    - [Constructors](#constructors-5)
    - [Methods](#methods-5)
  - [Class: PeerTrackerInfoSnapshot](#class-peertrackerinfosnapshot)
    - [Constructors](#constructors-6)
    - [Properties](#properties-4)
    - [Methods](#methods-6)
  - [Class: SyncingInfoSnapshot](#class-syncinginfosnapshot)
    - [Constructors](#constructors-7)
    - [Properties](#properties-5)
    - [Methods](#methods-7)
- [Enumerations](#enumerations)
  - [Enumeration: Network](#enumeration-network)
    - [Enumeration Members](#enumeration-members)
- [Functions](#functions)
  - [Function: setup\_logging()](#function-setup%5C_logging)
    - [Returns](#returns)
    - [Defined in](#defined-in)
- [@fl0rek/lumina-node-wasm](#fl0reklumina-node-wasm)
  - [Enumerations](#enumerations-1)
  - [Classes](#classes-1)
  - [Functions](#functions-1)

<!-- END doctoc generated TOC please keep comment here to allow auto update -->


<a name="readmemd"></a>

**@fl0rek/lumina-node-wasm** • [**Docs**](#globalsmd)

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

# Classes


<a name="classesblockrangemd"></a>

[**@fl0rek/lumina-node-wasm**](#readmemd) • **Docs**

***

[@fl0rek/lumina-node-wasm](#globalsmd) / BlockRange

## Class: BlockRange

### Constructors

#### new BlockRange()

> **new BlockRange**(): [`BlockRange`](#classesblockrangemd)

##### Returns

[`BlockRange`](#classesblockrangemd)

### Properties

#### end

> **end**: `bigint`

##### Defined in

lumina\_node\_wasm.d.ts:42

***

#### start

> **start**: `bigint`

##### Defined in

lumina\_node\_wasm.d.ts:45

### Methods

#### free()

> **free**(): `void`

##### Returns

`void`

##### Defined in

lumina\_node\_wasm.d.ts:39

***

#### toJSON()

> **toJSON**(): `Object`

* Return copy of self without private attributes.

##### Returns

`Object`

##### Defined in

lumina\_node\_wasm.d.ts:34

***

#### toString()

> **toString**(): `string`

Return stringified version of self.

##### Returns

`string`

##### Defined in

lumina\_node\_wasm.d.ts:38


<a name="classesconnectioncounterssnapshotmd"></a>

[**@fl0rek/lumina-node-wasm**](#readmemd) • **Docs**

***

[@fl0rek/lumina-node-wasm](#globalsmd) / ConnectionCountersSnapshot

## Class: ConnectionCountersSnapshot

### Constructors

#### new ConnectionCountersSnapshot()

> **new ConnectionCountersSnapshot**(): [`ConnectionCountersSnapshot`](#classesconnectioncounterssnapshotmd)

##### Returns

[`ConnectionCountersSnapshot`](#classesconnectioncounterssnapshotmd)

### Properties

#### num\_connections

> **num\_connections**: `number`

##### Defined in

lumina\_node\_wasm.d.ts:61

***

#### num\_established

> **num\_established**: `number`

##### Defined in

lumina\_node\_wasm.d.ts:64

***

#### num\_established\_incoming

> **num\_established\_incoming**: `number`

##### Defined in

lumina\_node\_wasm.d.ts:67

***

#### num\_established\_outgoing

> **num\_established\_outgoing**: `number`

##### Defined in

lumina\_node\_wasm.d.ts:70

***

#### num\_pending

> **num\_pending**: `number`

##### Defined in

lumina\_node\_wasm.d.ts:73

***

#### num\_pending\_incoming

> **num\_pending\_incoming**: `number`

##### Defined in

lumina\_node\_wasm.d.ts:76

***

#### num\_pending\_outgoing

> **num\_pending\_outgoing**: `number`

##### Defined in

lumina\_node\_wasm.d.ts:79

### Methods

#### free()

> **free**(): `void`

##### Returns

`void`

##### Defined in

lumina\_node\_wasm.d.ts:58

***

#### toJSON()

> **toJSON**(): `Object`

* Return copy of self without private attributes.

##### Returns

`Object`

##### Defined in

lumina\_node\_wasm.d.ts:53

***

#### toString()

> **toString**(): `string`

Return stringified version of self.

##### Returns

`string`

##### Defined in

lumina\_node\_wasm.d.ts:57


<a name="classesnetworkinfosnapshotmd"></a>

[**@fl0rek/lumina-node-wasm**](#readmemd) • **Docs**

***

[@fl0rek/lumina-node-wasm](#globalsmd) / NetworkInfoSnapshot

## Class: NetworkInfoSnapshot

### Constructors

#### new NetworkInfoSnapshot()

> **new NetworkInfoSnapshot**(): [`NetworkInfoSnapshot`](#classesnetworkinfosnapshotmd)

##### Returns

[`NetworkInfoSnapshot`](#classesnetworkinfosnapshotmd)

### Properties

#### connection\_counters

> **connection\_counters**: [`ConnectionCountersSnapshot`](#classesconnectioncounterssnapshotmd)

##### Defined in

lumina\_node\_wasm.d.ts:95

***

#### num\_peers

> **num\_peers**: `number`

##### Defined in

lumina\_node\_wasm.d.ts:98

### Methods

#### free()

> **free**(): `void`

##### Returns

`void`

##### Defined in

lumina\_node\_wasm.d.ts:92

***

#### toJSON()

> **toJSON**(): `Object`

* Return copy of self without private attributes.

##### Returns

`Object`

##### Defined in

lumina\_node\_wasm.d.ts:87

***

#### toString()

> **toString**(): `string`

Return stringified version of self.

##### Returns

`string`

##### Defined in

lumina\_node\_wasm.d.ts:91


<a name="classesnodeclientmd"></a>

[**@fl0rek/lumina-node-wasm**](#readmemd) • **Docs**

***

[@fl0rek/lumina-node-wasm](#globalsmd) / NodeClient

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

lumina\_node\_wasm.d.ts:113

### Methods

#### addConnectionToWorker()

> **addConnectionToWorker**(`port`): `Promise`\<`void`\>

Establish a new connection to the existing worker over provided port

##### Parameters

• **port**: `any`

##### Returns

`Promise`\<`void`\>

##### Defined in

lumina\_node\_wasm.d.ts:119

***

#### close()

> **close**(): `Promise`\<`void`\>

Requests SharedWorker running lumina to close. Any events received afterwards wont
be processed and new NodeClient needs to be created to restart a node.

##### Returns

`Promise`\<`void`\>

##### Defined in

lumina\_node\_wasm.d.ts:282

***

#### connectedPeers()

> **connectedPeers**(): `Promise`\<`any`[]\>

Get all the peers that node is connected to.

##### Returns

`Promise`\<`any`[]\>

##### Defined in

lumina\_node\_wasm.d.ts:165

***

#### eventsChannel()

> **eventsChannel**(): `Promise`\<`BroadcastChannel`\>

Returns a [`BroadcastChannel`] for events generated by [`Node`].

##### Returns

`Promise`\<`BroadcastChannel`\>

##### Defined in

lumina\_node\_wasm.d.ts:287

***

#### free()

> **free**(): `void`

##### Returns

`void`

##### Defined in

lumina\_node\_wasm.d.ts:107

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

lumina\_node\_wasm.d.ts:240

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

lumina\_node\_wasm.d.ts:249

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

lumina\_node\_wasm.d.ts:267

***

#### getLocalHeadHeader()

> **getLocalHeadHeader**(): `Promise`\<`any`\>

Get the latest locally synced header.

Returns a javascript object with given structure:
https://docs.rs/celestia-types/latest/celestia_types/struct.ExtendedHeader.html

##### Returns

`Promise`\<`any`\>

##### Defined in

lumina\_node\_wasm.d.ts:231

***

#### getNetworkHeadHeader()

> **getNetworkHeadHeader**(): `Promise`\<`any`\>

Get the latest header announced in the network.

Returns a javascript object with given structure:
https://docs.rs/celestia-types/latest/celestia_types/struct.ExtendedHeader.html

##### Returns

`Promise`\<`any`\>

##### Defined in

lumina\_node\_wasm.d.ts:223

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

lumina\_node\_wasm.d.ts:276

***

#### isRunning()

> **isRunning**(): `Promise`\<`boolean`\>

Check whether Lumina is currently running

##### Returns

`Promise`\<`boolean`\>

##### Defined in

lumina\_node\_wasm.d.ts:124

***

#### listeners()

> **listeners**(): `Promise`\<`any`[]\>

Get all the multiaddresses on which the node listens.

##### Returns

`Promise`\<`any`[]\>

##### Defined in

lumina\_node\_wasm.d.ts:160

***

#### localPeerId()

> **localPeerId**(): `Promise`\<`string`\>

Get node's local peer ID.

##### Returns

`Promise`\<`string`\>

##### Defined in

lumina\_node\_wasm.d.ts:135

***

#### networkInfo()

> **networkInfo**(): `Promise`\<[`NetworkInfoSnapshot`](#classesnetworkinfosnapshotmd)\>

Get current network info.

##### Returns

`Promise`\<[`NetworkInfoSnapshot`](#classesnetworkinfosnapshotmd)\>

##### Defined in

lumina\_node\_wasm.d.ts:155

***

#### peerTrackerInfo()

> **peerTrackerInfo**(): `Promise`\<[`PeerTrackerInfoSnapshot`](#classespeertrackerinfosnapshotmd)\>

Get current [`PeerTracker`] info.

##### Returns

`Promise`\<[`PeerTrackerInfoSnapshot`](#classespeertrackerinfosnapshotmd)\>

##### Defined in

lumina\_node\_wasm.d.ts:140

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

lumina\_node\_wasm.d.ts:189

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

lumina\_node\_wasm.d.ts:198

***

#### requestHeadHeader()

> **requestHeadHeader**(): `Promise`\<`any`\>

Request the head header from the network.

Returns a javascript object with given structure:
https://docs.rs/celestia-types/latest/celestia_types/struct.ExtendedHeader.html

##### Returns

`Promise`\<`any`\>

##### Defined in

lumina\_node\_wasm.d.ts:180

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

lumina\_node\_wasm.d.ts:210

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

lumina\_node\_wasm.d.ts:172

***

#### start()

> **start**(`config`): `Promise`\<`void`\>

Start a node with the provided config, if it's not running

##### Parameters

• **config**: [`NodeConfig`](#classesnodeconfigmd)

##### Returns

`Promise`\<`void`\>

##### Defined in

lumina\_node\_wasm.d.ts:130

***

#### syncerInfo()

> **syncerInfo**(): `Promise`\<[`SyncingInfoSnapshot`](#classessyncinginfosnapshotmd)\>

Get current header syncing info.

##### Returns

`Promise`\<[`SyncingInfoSnapshot`](#classessyncinginfosnapshotmd)\>

##### Defined in

lumina\_node\_wasm.d.ts:215

***

#### waitConnected()

> **waitConnected**(): `Promise`\<`void`\>

Wait until the node is connected to at least 1 peer.

##### Returns

`Promise`\<`void`\>

##### Defined in

lumina\_node\_wasm.d.ts:145

***

#### waitConnectedTrusted()

> **waitConnectedTrusted**(): `Promise`\<`void`\>

Wait until the node is connected to at least 1 trusted peer.

##### Returns

`Promise`\<`void`\>

##### Defined in

lumina\_node\_wasm.d.ts:150


<a name="classesnodeconfigmd"></a>

[**@fl0rek/lumina-node-wasm**](#readmemd) • **Docs**

***

[@fl0rek/lumina-node-wasm](#globalsmd) / NodeConfig

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

lumina\_node\_wasm.d.ts:311

***

#### network

> **network**: [`Network`](#enumerationsnetworkmd)

A network to connect to.

##### Defined in

lumina\_node\_wasm.d.ts:315

### Methods

#### free()

> **free**(): `void`

##### Returns

`void`

##### Defined in

lumina\_node\_wasm.d.ts:301

***

#### toJSON()

> **toJSON**(): `Object`

* Return copy of self without private attributes.

##### Returns

`Object`

##### Defined in

lumina\_node\_wasm.d.ts:296

***

#### toString()

> **toString**(): `string`

Return stringified version of self.

##### Returns

`string`

##### Defined in

lumina\_node\_wasm.d.ts:300

***

#### default()

> `static` **default**(`network`): [`NodeConfig`](#classesnodeconfigmd)

Get the configuration with default bootnodes for provided network

##### Parameters

• **network**: [`Network`](#enumerationsnetworkmd)

##### Returns

[`NodeConfig`](#classesnodeconfigmd)

##### Defined in

lumina\_node\_wasm.d.ts:307


<a name="classesnodeworkermd"></a>

[**@fl0rek/lumina-node-wasm**](#readmemd) • **Docs**

***

[@fl0rek/lumina-node-wasm](#globalsmd) / NodeWorker

## Class: NodeWorker

### Constructors

#### new NodeWorker()

> **new NodeWorker**(`port_like_object`): [`NodeWorker`](#classesnodeworkermd)

##### Parameters

• **port\_like\_object**: `any`

##### Returns

[`NodeWorker`](#classesnodeworkermd)

##### Defined in

lumina\_node\_wasm.d.ts:324

### Methods

#### free()

> **free**(): `void`

##### Returns

`void`

##### Defined in

lumina\_node\_wasm.d.ts:320

***

#### run()

> **run**(): `Promise`\<`void`\>

##### Returns

`Promise`\<`void`\>

##### Defined in

lumina\_node\_wasm.d.ts:328


<a name="classespeertrackerinfosnapshotmd"></a>

[**@fl0rek/lumina-node-wasm**](#readmemd) • **Docs**

***

[@fl0rek/lumina-node-wasm](#globalsmd) / PeerTrackerInfoSnapshot

## Class: PeerTrackerInfoSnapshot

### Constructors

#### new PeerTrackerInfoSnapshot()

> **new PeerTrackerInfoSnapshot**(): [`PeerTrackerInfoSnapshot`](#classespeertrackerinfosnapshotmd)

##### Returns

[`PeerTrackerInfoSnapshot`](#classespeertrackerinfosnapshotmd)

### Properties

#### num\_connected\_peers

> **num\_connected\_peers**: `bigint`

##### Defined in

lumina\_node\_wasm.d.ts:344

***

#### num\_connected\_trusted\_peers

> **num\_connected\_trusted\_peers**: `bigint`

##### Defined in

lumina\_node\_wasm.d.ts:347

### Methods

#### free()

> **free**(): `void`

##### Returns

`void`

##### Defined in

lumina\_node\_wasm.d.ts:341

***

#### toJSON()

> **toJSON**(): `Object`

* Return copy of self without private attributes.

##### Returns

`Object`

##### Defined in

lumina\_node\_wasm.d.ts:336

***

#### toString()

> **toString**(): `string`

Return stringified version of self.

##### Returns

`string`

##### Defined in

lumina\_node\_wasm.d.ts:340


<a name="classessyncinginfosnapshotmd"></a>

[**@fl0rek/lumina-node-wasm**](#readmemd) • **Docs**

***

[@fl0rek/lumina-node-wasm](#globalsmd) / SyncingInfoSnapshot

## Class: SyncingInfoSnapshot

### Constructors

#### new SyncingInfoSnapshot()

> **new SyncingInfoSnapshot**(): [`SyncingInfoSnapshot`](#classessyncinginfosnapshotmd)

##### Returns

[`SyncingInfoSnapshot`](#classessyncinginfosnapshotmd)

### Properties

#### stored\_headers

> **stored\_headers**: [`BlockRange`](#classesblockrangemd)[]

##### Defined in

lumina\_node\_wasm.d.ts:363

***

#### subjective\_head

> **subjective\_head**: `bigint`

##### Defined in

lumina\_node\_wasm.d.ts:366

### Methods

#### free()

> **free**(): `void`

##### Returns

`void`

##### Defined in

lumina\_node\_wasm.d.ts:360

***

#### toJSON()

> **toJSON**(): `Object`

* Return copy of self without private attributes.

##### Returns

`Object`

##### Defined in

lumina\_node\_wasm.d.ts:355

***

#### toString()

> **toString**(): `string`

Return stringified version of self.

##### Returns

`string`

##### Defined in

lumina\_node\_wasm.d.ts:359

# Enumerations


<a name="enumerationsnetworkmd"></a>

[**@fl0rek/lumina-node-wasm**](#readmemd) • **Docs**

***

[@fl0rek/lumina-node-wasm](#globalsmd) / Network

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

[**@fl0rek/lumina-node-wasm**](#readmemd) • **Docs**

***

[@fl0rek/lumina-node-wasm](#globalsmd) / setup\_logging

## Function: setup\_logging()

> **setup\_logging**(): `void`

Set up a logging layer that direct logs to the browser's console.

### Returns

`void`

### Defined in

lumina\_node\_wasm.d.ts:6


<a name="globalsmd"></a>

[**@fl0rek/lumina-node-wasm**](#readmemd) • **Docs**

***

# @fl0rek/lumina-node-wasm

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
