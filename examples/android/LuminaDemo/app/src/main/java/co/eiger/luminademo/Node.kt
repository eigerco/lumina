package co.eiger.luminademo

import android.util.Log
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.tooling.preview.Preview
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import fr.acinq.secp256k1.Hex
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.cancel
import kotlinx.coroutines.channels.awaitClose
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.callbackFlow
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import uniffi.celestia_grpc.TxClient
import uniffi.celestia_grpc.parseBech32Address
import uniffi.celestia_types.AppVersion
import uniffi.celestia_types.Blob
import uniffi.celestia_types.Namespace
import uniffi.lumina_node.Network
import uniffi.lumina_node.NetworkId
import uniffi.lumina_node_uniffi.BlockRange
import uniffi.lumina_node_uniffi.LuminaNode
import uniffi.lumina_node_uniffi.NodeConfig
import java.math.BigInteger
import java.security.AlgorithmParameters
import java.security.KeyFactory
import java.security.spec.ECGenParameterSpec
import java.security.spec.ECParameterSpec
import java.security.spec.ECPrivateKeySpec

@Preview
@Composable
fun LuminaUi(modifier: Modifier = Modifier) {
    val dbPath = LocalContext.current.filesDir.path;

    var config by remember { mutableStateOf<NodeConfig?>(null) }

    val networks = listOf(
        Network.Mainnet,
        Network.Mocha,
        Network.Arabica,
        Network.Custom(NetworkId("private"))
    )

    val onNetworkSelected = { network: Network ->
        Log.d("MAIN", "Clicked on $network")

        config = NodeConfig(
            basePath = dbPath,
            network = network,
            bootnodes = null,
            syncingWindowSecs = null,
            pruningDelaySecs = null,
            batchSize = null,
            ed25519SecretKeyBytes = null,
        )
        Log.d("MAIN", "Clicked Config: $config")
    }

    val resetConfig = {
        config = null
    }

    if (config == null) {
        Box(modifier) {
            Column(modifier, horizontalAlignment = Alignment.CenterHorizontally) {
                Text(
                    "Select network",
                    style = TextStyle(fontSize = 20.sp),
                    modifier = modifier.padding(20.dp)
                )
                networks.forEach { network ->
                    Button(onClick = { onNetworkSelected(network) }) {
                        Text(text = network.toNetworkName())
                    }
                }
            }
        }
    } else {
        LuminaStatus(config!!, resetConfig, modifier)
    }
}

@OptIn(ExperimentalStdlibApi::class)
@Composable
fun LuminaStatus(
    config: NodeConfig,
    resetConfig: () -> Unit,
    modifier: Modifier = Modifier
) {
    val coroutineScope = rememberCoroutineScope()
    var lumina by remember { mutableStateOf<LuminaNode?>(null) }
    var luminaStats by remember { mutableStateOf<LuminaStats?>(null) }

    LaunchedEffect(config) {
        lumina = LuminaNode(config)
        coroutineScope.launch {
            val s = lumina!!.start()
            Log.d("MAIN", "Started lumina: $s")
        }

        LuminaPoller(dispatcher = Dispatchers.Main, fetchStats = {
            val peerInfo = lumina!!.peerTrackerInfo()
            val syncerInfo = lumina!!.syncerInfo()
            LuminaStats(
                networkHeight = syncerInfo.subjectiveHead,
                ranges = syncerInfo.storedHeaders,
                connectedPeers = peerInfo.numConnectedPeers,
                trustedPeers = peerInfo.numConnectedTrustedPeers,
            )
        }).poll(1000).collect { stats ->
            luminaStats = stats
            Log.d("MAIN", "Stats: $stats")
        }
    }

    if (luminaStats == null) {
        Box(modifier = Modifier.fillMaxSize(),
            contentAlignment = Alignment.Center,
            ) {
            Text(
                text = "Connecting to Celestia...",
                modifier = Modifier.Companion
                    .fillMaxSize()
                    .padding(all = 50.dp),
                textAlign = TextAlign.Companion.Center
            )
            Box(
                contentAlignment = Alignment.Companion.Center,
                modifier = Modifier.Companion.fillMaxSize()
            ) {
                CircularProgressIndicator()
            }
        }
        return
    }

    val rowModifier = Modifier.Companion
        .fillMaxWidth()
        .padding(8.dp)
    Column(modifier = modifier.fillMaxWidth()) {
        Row(modifier = rowModifier, horizontalArrangement = Arrangement.Center) {
            Text(text = "Lumina Node started", textAlign = TextAlign.Companion.Center)
        }
        Row(modifier = rowModifier) {
            Text(
                "Network",
                modifier = Modifier.Companion.weight(1f),
                textAlign = TextAlign.Companion.Center
            )
            Text(
                config.network.toNetworkName(),
                modifier = Modifier.Companion.weight(1f),
                textAlign = TextAlign.Companion.Center
            )
        }
        Row(modifier = rowModifier) {
            Text(
                "Network Height",
                modifier = Modifier.Companion.weight(1f),
                textAlign = TextAlign.Companion.Center
            )
            Text(
                luminaStats?.networkHeight.toString(),
                modifier = Modifier.Companion.weight(1f),
                textAlign = TextAlign.Companion.Center
            )
        }
        Row(modifier = rowModifier) {
            Text(
                "Connected Peers",
                modifier = Modifier.Companion.weight(1f),
                textAlign = TextAlign.Companion.Center
            )
            Text(
                luminaStats?.connectedPeers.toString(),
                modifier = Modifier.Companion.weight(1f),
                textAlign = TextAlign.Companion.Center
            )
        }
        Row(modifier = rowModifier) {
            Text(
                "Trusted Peers",
                modifier = Modifier.Companion.weight(1f),
                textAlign = TextAlign.Companion.Center
            )
            Text(
                luminaStats?.trustedPeers.toString(),
                modifier = Modifier.Companion.weight(1f),
                textAlign = TextAlign.Companion.Center
            )
        }

        Row(modifier = rowModifier, horizontalArrangement = Arrangement.Center) {
            Text(text = "Synced Ranges", textAlign = TextAlign.Companion.Center)
        }

        luminaStats?.ranges?.forEach {
            Row(modifier = rowModifier) {
                Text(
                    it.start.toString(),
                    modifier = Modifier.Companion.weight(1f),
                    textAlign = TextAlign.Companion.Right
                )
                Text(
                    " - ",
                    modifier = Modifier.Companion.weight(0.1f),
                    textAlign = TextAlign.Companion.Center
                )
                Text(
                    it.end.toString(),
                    modifier = Modifier.Companion.weight(1f),
                    textAlign = TextAlign.Companion.Left
                )
            }
        }

        Row(modifier = rowModifier, horizontalArrangement = Arrangement.Center) {
            Button(onClick = {
                coroutineScope.launch(Dispatchers.Main) { lumina?.stop(); resetConfig() }
            }) {
                Text(text = "Stop Node")
            }
        }
    }
}

data class LuminaStats(
    val networkHeight: ULong,
    val ranges: List<BlockRange>,
    val connectedPeers: ULong,
    val trustedPeers: ULong
)

class LuminaPoller<T> (
    private val dispatcher: CoroutineDispatcher,
    private val fetchStats: suspend () -> T,
) {
    private var job: Job? = null

    fun poll(delay: Long) = callbackFlow {
        job = launch(dispatcher) {
            while (isActive) {
                val stats = fetchStats()
                trySend(stats)
                delay(delay)
            }
        }
        awaitClose {
            cancel()
        }
    }

    fun cancel() {
        job?.cancel()
        job = null
    }
}

fun Network.toNetworkName(): String {
    return when (this) {
        is Network.Mocha -> "Mocha"
        is Network.Mainnet -> "Mainnet"
        is Network.Arabica -> "Arabica"
        is Network.Custom -> "Custom( id = ${v1.id} )"
    }
}