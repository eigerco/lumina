package co.eiger.luminademo

import androidx.test.ext.junit.runners.AndroidJUnit4
import kotlinx.coroutines.test.runTest
import org.junit.Test
import org.junit.runner.RunWith
import uniffi.celestia_grpc.GrpcClientBuilder

// Ip of the host machine when running on an Android emulator, see https://developer.android.com/studio/run/emulator-networking
const val GRPC_URL = "http://10.0.2.2:19090"

@RunWith(AndroidJUnit4::class)
class GrpcClientTest {
    @Test
    fun getMinGasPrice() = runTest {
        val grpc = GrpcClientBuilder.
            withUrl(GRPC_URL).build()
        val nodeConfig = grpc.getNodeConfig()
    }
}
