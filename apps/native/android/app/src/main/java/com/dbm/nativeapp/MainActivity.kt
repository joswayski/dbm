package com.dbm.nativeapp

import android.os.Bundle
import android.view.WindowManager
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.foundation.*
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.font.Font
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.unit.dp
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleEventObserver
import androidx.lifecycle.compose.LocalLifecycleOwner
import androidx.lifecycle.ViewModel
import androidx.lifecycle.ViewModelProvider
import androidx.lifecycle.viewmodel.compose.viewModel
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.contentOrNull

class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState); enableEdgeToEdge()
        val demo = BuildConfig.ALLOW_DEMO && intent.getBooleanExtra("demo", false)
        if (!demo) window.addFlags(WindowManager.LayoutParams.FLAG_SECURE)
        val factory = object : ViewModelProvider.Factory { override fun <T : ViewModel> create(modelClass: Class<T>): T = DbmViewModel(application, demo) as T }
        setContent { DbmApp(viewModel(factory = factory)) }
    }
}

private val Bg = Color(0xff161618); private val Chrome = Color(0xff1c1c1e); private val Control = Color(0xff2a2a2d)
private val Text = Color(0xffe8e8ea); private val Muted = Color(0xffa0a0a6); private val Accent = Color(0xff4c9aff)
private val Danger = Color(0xffff8a80)

@Composable fun DbmApp(vm: DbmViewModel = viewModel()) {
    val owner = LocalLifecycleOwner.current
    DisposableEffect(owner) {
        val observer = LifecycleEventObserver { _, event ->
            if (event == Lifecycle.Event.ON_STOP) vm.background()
            if (event == Lifecycle.Event.ON_START) vm.foreground()
        }
        owner.lifecycle.addObserver(observer); onDispose { owner.lifecycle.removeObserver(observer) }
    }
    val fonts = FontFamily(Font(R.font.geist))
    MaterialTheme(colorScheme = darkColorScheme(background = Bg, surface = Chrome, primary = Accent, onBackground = Text, onSurface = Text, error = Danger), typography = Typography().run { copy(bodyLarge = bodyLarge.copy(fontFamily = fonts), bodyMedium = bodyMedium.copy(fontFamily = fonts), titleLarge = titleLarge.copy(fontFamily = fonts)) }) {
        Surface(Modifier.fillMaxSize()) { Column(Modifier.safeDrawingPadding()) {
            if (vm.demo) Text("DEMO — local Rust fixture", Modifier.fillMaxWidth().background(Control).padding(8.dp))
            Status(vm.state)
            when (vm.state.screen) {
            Screen.Connections -> Connections(vm)
            Screen.Form -> Form(vm)
            Screen.Query -> Query(vm)
            Screen.Explorer -> Explorer(vm)
            Screen.Browse -> Browse(vm)
        } } }
    }
}

@Composable private fun Status(state: UiState) { Column { if (state.busy) LinearProgressIndicator(Modifier.fillMaxWidth().testTag("loading")); state.error?.let { Text(it, color = Danger, modifier = Modifier.padding(12.dp).testTag("error")) }; state.message?.let { Text(it, color = Color(0xff5ad394), modifier = Modifier.padding(12.dp)) } } }

@Composable private fun Connections(vm: DbmViewModel) = Column(Modifier.fillMaxSize().padding(16.dp)) {
    var deleting by remember { mutableStateOf<Profile?>(null) }
    Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween) { Text("Connections", style = MaterialTheme.typography.titleLarge); Button(onClick = vm::add, enabled = !vm.state.busy, modifier = Modifier.testTag("add")) { Text("Add") } }
    if (!vm.state.busy && vm.state.profiles.isEmpty()) Text("No saved connections", color = Muted, modifier = Modifier.padding(top = 32.dp).testTag("empty"))
    LazyColumn { items(vm.state.profiles, key = { it.id }) { profile -> Row(Modifier.fillMaxWidth().clickable(enabled = !vm.state.busy) { vm.select(profile) }.padding(vertical = 14.dp), horizontalArrangement = Arrangement.SpaceBetween) { Column(Modifier.weight(1f)) { Text(profile.name, color = runCatching { Color(android.graphics.Color.parseColor(profile.color ?: "#4c9aff")) }.getOrDefault(Accent)); Text("${profile.engine} · ${profile.host}:${profile.port}", color = Muted) }; TextButton(onClick = { deleting = profile }, enabled = !vm.state.busy) { Text("Delete", color = Danger) } } } }
    deleting?.let { profile ->
        AlertDialog(onDismissRequest = { deleting = null }, title = { Text("Delete ${profile.name}?") },
            text = { Text("This removes the saved connection and its local query history.") },
            confirmButton = { TextButton(onClick = { deleting = null; vm.delete(profile.id) }) { Text("Delete connection", color = Danger) } },
            dismissButton = { TextButton(onClick = { deleting = null }) { Text("Cancel") } })
    }
}

@Composable private fun Form(vm: DbmViewModel) { val d = vm.state.draft; LazyColumn(Modifier.fillMaxSize().padding(16.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
    item { TextButton(vm::cancelForm, enabled = !vm.state.busy) { Text("Cancel") }; Text(if (d.id == null) "New connection" else "Connect · ${d.name}", style = MaterialTheme.typography.titleLarge); Text("Read-only · TLS required. Enter your password for each session. It is not saved on disk.", color = Muted) }
    item { Row(Modifier.horizontalScroll(rememberScrollState())) { listOf("postgres", "mysql", "redis").forEach { e -> FilterChip(selected = d.engine == e, onClick = { vm.editDraft { it.copy(engine=e, port=if(e=="postgres") "5432" else if(e=="mysql") "3306" else "6379", database=if(e=="redis") "0" else "") } }, label = { Text(e) }); Spacer(Modifier.width(6.dp)) } } }
    item { Field("Name", d.name) { vm.editDraft { x -> x.copy(name=it) } }; Field("Color", d.color) { vm.editDraft { x -> x.copy(color=it) } }; Field("Host", d.host) { vm.editDraft { x -> x.copy(host=it) } }; Field("Port", d.port) { vm.editDraft { x -> x.copy(port=it) } }; Field("User", d.username) { vm.editDraft { x -> x.copy(username=it) } }; Field(if(d.engine=="redis") "Database index" else "Database", d.database) { vm.editDraft { x -> x.copy(database=it) } }; OutlinedTextField(d.password, { vm.editDraft { x -> x.copy(password=it) } }, label={Text("Password (session only)")}, visualTransformation=PasswordVisualTransformation(), modifier=Modifier.fillMaxWidth().testTag("password")) }
    item { Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) { OutlinedButton(onClick=vm::test, enabled=!vm.state.busy) { Text("Test") }; OutlinedButton(onClick={vm.save()}, enabled=!vm.state.busy, modifier=Modifier.testTag("save")) { Text("Save") }; Button(onClick={vm.save(true)}, enabled=!vm.state.busy) { Text("Connect") } } }
} }
@Composable private fun Field(label:String, value:String, changed:(String)->Unit) { OutlinedTextField(value, changed, label={Text(label)}, singleLine=true, modifier=Modifier.fillMaxWidth()) }

@Composable private fun Query(vm: DbmViewModel) = Column(Modifier.fillMaxSize()) {
    val s=vm.state; Row(Modifier.fillMaxWidth().background(Chrome).horizontalScroll(rememberScrollState()).padding(8.dp), horizontalArrangement=Arrangement.spacedBy(8.dp)) { Text(s.active?.name ?: "Query", modifier=Modifier.padding(10.dp)); s.databases.filter { it.isConnectable }.forEach { db -> FilterChip(selected=s.database==db.name, onClick={vm.selectDatabase(db.name)}, label={Text(db.name)}) } }
    Row { TextButton(vm::disconnect, enabled=!s.busy) { Text("Disconnect") }; TextButton(vm::explorer, enabled=!s.busy) { Text("Explorer") } }
    OutlinedTextField(s.sql, vm::setSql, textStyle=MaterialTheme.typography.bodyMedium.copy(fontFamily=FontFamily(Font(R.font.geist_mono))), modifier=Modifier.fillMaxWidth().height(140.dp).padding(horizontal=10.dp).testTag("editor"))
    Button(vm::query, enabled=!s.busy && s.sql.isNotBlank(), modifier=Modifier.padding(10.dp).testTag("run")) { Text("Run / Refresh · max 1000") }
    DataGrid(s.columns,s.rows,Modifier.weight(1f))
}
private fun flatten(nodes:List<SchemaNode>):List<SchemaNode> = nodes.flatMap { listOf(it)+flatten(it.children) }

@Composable private fun Explorer(vm: DbmViewModel) = Column(Modifier.fillMaxSize().padding(12.dp)) {
    Row { TextButton(vm::backToQuery) { Text("Query") }; TextButton(vm::refreshSchema, enabled=!vm.state.busy) { Text("Refresh schema") } }
    LazyColumn(Modifier.testTag("schema")) { items(flatten(vm.state.tree)) { n -> if (n.table != null) Text(n.name, Modifier.fillMaxWidth().clickable(enabled=!vm.state.busy) { vm.browse(n.schema ?: "", n.table) }.padding(vertical=16.dp)) else Text(n.name, color=Muted, modifier=Modifier.padding(vertical=8.dp)) } }
}

@Composable private fun Browse(vm: DbmViewModel) = Column(Modifier.fillMaxSize().padding(10.dp)) {
    val s = vm.state
    Text(s.table?.let { "${it.first}.${it.second}" } ?: "Table")
    Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
        TextButton(vm::backToQuery) { Text("Query") }
        OutlinedButton({ s.table?.let { vm.browse(it.first, it.second, s.offset) } }, enabled = !s.busy) { Text("Refresh") }
    }
    DataGrid(s.columns, s.rows, Modifier.weight(1f))
    Text(if (s.rows.isEmpty()) "0 rows" else "Rows ${s.offset + 1}–${s.offset + s.rows.size}", modifier = Modifier.padding(12.dp))
    Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
        OutlinedButton({ s.table?.let { vm.browse(it.first, it.second, (s.offset - TABLE_PAGE_SIZE).coerceAtLeast(0)) } }, enabled = s.offset > 0 && !s.busy) { Text("Previous") }
        Button({ s.table?.let { vm.browse(it.first, it.second, s.offset + TABLE_PAGE_SIZE) } }, enabled = s.hasMore && !s.busy, modifier = Modifier.testTag("next")) { Text("Next") }
    }
}

@Composable private fun DataGrid(columns:List<String>, rows:List<List<kotlinx.serialization.json.JsonElement>>, modifier:Modifier=Modifier) { if(columns.isEmpty()) { Box(modifier.fillMaxWidth()){Text("No results",color=Muted,modifier=Modifier.padding(16.dp))}; return }; Box(modifier.horizontalScroll(rememberScrollState()).verticalScroll(rememberScrollState()).testTag("results")) { Column { Row(Modifier.background(Color(0xff1a1a1c))) { columns.forEach { Cell(it,true) } }; rows.forEach { row -> Row { row.forEach { value -> Cell(if(value is JsonNull) "NULL" else (value as? JsonPrimitive)?.contentOrNull ?: value.toString(),false) } } } } } }
@Composable private fun Cell(value:String, header:Boolean) { Text(value, color=if(header) Text else Color(0xffc7c7cc), fontFamily=FontFamily(Font(R.font.geist_mono)), modifier=Modifier.width(160.dp).height(48.dp).border(0.5.dp,Control).padding(8.dp), maxLines=2) }
