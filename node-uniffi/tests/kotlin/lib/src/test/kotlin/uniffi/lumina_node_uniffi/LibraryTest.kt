package uniffi.lumina_node_uniffi

import kotlinx.coroutines.test.runTest
import kotlinx.serialization.json.*
import kotlin.test.Test
import kotlin.test.assertEquals
import uniffi.lumina_node_uniffi.*
import uniffi.lumina_node.*

class LibraryTest {
    @Test
    fun testRequestHeader() = runTest {
        var config = NodeConfig(null, Network.Mainnet, null, null, null, null)
        var node = LuminaNode(config)

        node.start()
        node.waitConnectedTrusted()

        var headStr = node.requestHeadHeader()
        var head: JsonObject = Json.decodeFromString<JsonObject>(headStr)
        var headHeader = head.get("header")!!.jsonObject
        var headHeight = headHeader.get("height")!!.jsonPrimitive.content.toULong()

        var prevStr = node.requestHeaderByHeight(headHeight - 1.toULong())
        var prev: JsonObject = Json.decodeFromString<JsonObject>(prevStr)
        var prevHash = prev.get("commit")!!.jsonObject.get("block_id")!!.jsonObject.get("hash")!!

        var expectedPrevHash = headHeader.get("last_block_id")!!.jsonObject.get("hash")!!
        assertEquals(prevHash, expectedPrevHash)
    }
}
