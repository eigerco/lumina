package co.eiger.luminademo

import android.os.Bundle
import android.util.Log
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.CloudSync
import androidx.compose.material.icons.filled.WifiCalling
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.tooling.preview.Preview
import co.eiger.luminademo.ui.theme.LuminaDemoTheme
import androidx.navigation.NavHostController
import androidx.navigation.compose.NavHost
import androidx.navigation.compose.composable
import androidx.navigation.compose.rememberNavController
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.material3.PrimaryTabRow
import androidx.compose.material3.Tab
import androidx.compose.ui.unit.dp

enum class Destination(
    val route: String,
    val label: String,
    val icon: ImageVector,
    val contentDescription: String,
) {
    NODE(
        route = "node",
        label = "Node",
        icon = Icons.Default.CloudSync,
        contentDescription = "Light Node"
    ),
    GRPC(
        route = "grpc",
        label = "gRPC",
        icon = Icons.Default.WifiCalling,
        contentDescription = "gRPC"
    ),
}

@Composable
fun AppNavHost(
    navController: NavHostController,
    startDestination: Destination,
    modifier: Modifier = Modifier
) {
    NavHost(
        navController,
        startDestination = startDestination.route,
    ) {
        Destination.entries.forEach { destination ->
            composable(destination.route) {
                when (destination) {
                    // Are we supposed to add spacing for the navigation bar manually?
                    Destination.GRPC -> GrpcUi(modifier.padding(top = 100.dp))
                    Destination.NODE -> LuminaUi(modifier.padding(top = 100.dp))
                }
            }
        }
    }
}

class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)

        enableEdgeToEdge()

        Log.i("MAIN", "Starting Lumina Demo")

        setContent {
            LuminaDemoTheme {
                Scaffold(modifier = Modifier.fillMaxSize()) { innerPadding ->
                    ExamplesNavigation(modifier = Modifier.padding(innerPadding).fillMaxWidth())
                }
            }
        }
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Preview
@Composable
fun ExamplesNavigation(modifier: Modifier = Modifier) {
    val navController = rememberNavController()
    val startDestination = Destination.NODE
    var selectedDestination by rememberSaveable { mutableIntStateOf(startDestination.ordinal) }

    Scaffold(modifier = modifier) { contentPadding ->
        PrimaryTabRow(selectedTabIndex = selectedDestination, modifier = Modifier.padding(contentPadding)) {
            Destination.entries.forEachIndexed { index, destination ->
                Tab(
                    selected = selectedDestination == index,
                    onClick = {
                        navController.navigate(route = destination.route)
                        selectedDestination = index
                    },
                    text = {
                        Text(
                            text = destination.label,
                            maxLines = 2,
                            overflow = TextOverflow.Ellipsis
                        )
                    },
                    icon = {
                        Icon(
                            imageVector = destination.icon,
                            contentDescription = destination.contentDescription
                        )
                    }
                )
            }
        }
        AppNavHost(navController, startDestination)
    }
}