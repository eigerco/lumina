package co.eiger.luminademo

import androidx.test.ext.junit.runners.AndroidJUnit4
import kotlinx.coroutines.test.runTest
import org.junit.Test
import org.junit.runner.RunWith
import uniffi.lumina_node.Network
import uniffi.lumina_node_uniffi.LuminaNode
import uniffi.lumina_node_uniffi.NodeConfig
import kotlin.io.path.createTempDirectory

@RunWith(AndroidJUnit4::class)
class NodeTests {
    @Test
    fun identityRetention() = runTest {
        val dbPath = createTempDirectory().toString()
        val config = NodeConfig(
            basePath = dbPath,
            network = Network.Arabica,
            bootnodes = null,
            pruningWindowSecs = null,
            batchSize = null,
            ed25519SecretKeyBytes = null
        );
        val node0 = LuminaNode(config);
        node0.start();
        val peerId0 = node0.localPeerId();
        node0.stop();

        val node1 = LuminaNode(config);
        node1.start();
        val peerId1 = node1.localPeerId();
        node1.stop();

        assert(peerId0 == peerId1)
    }
}
