package com.dbm.nativeapp

import android.os.Bundle
import android.view.WindowManager
import androidx.activity.ComponentActivity
import androidx.activity.SystemBarStyle
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.foundation.*
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.drawscope.withTransform
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.Font
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleEventObserver
import androidx.lifecycle.compose.LocalLifecycleOwner
import androidx.lifecycle.ViewModel
import androidx.lifecycle.ViewModelProvider
import androidx.lifecycle.viewmodel.compose.viewModel
import kotlinx.coroutines.launch
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.contentOrNull

class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge(SystemBarStyle.dark(android.graphics.Color.TRANSPARENT), SystemBarStyle.dark(android.graphics.Color.TRANSPARENT))
        val demo = BuildConfig.ALLOW_DEMO && intent.getBooleanExtra("demo", false)
        if (!demo) window.addFlags(WindowManager.LayoutParams.FLAG_SECURE)
        val factory = object : ViewModelProvider.Factory {
            override fun <T : ViewModel> create(modelClass: Class<T>): T = DbmViewModel(application, demo) as T
        }
        setContent { DbmApp(viewModel(factory = factory)) }
    }
}

private val Bg = Color(0xff161618); private val Chrome = Color(0xff1c1c1e)
private val Sidebar = Color(0xff1f1f21); private val GridHeader = Color(0xff1a1a1c)
private val Control = Color(0xff2a2a2d); private val ControlActive = Color(0xff3a3a3d)
private val Border = Color(0xff2a2a2d); private val BorderStrong = Color(0xff353538)
private val TextStrong = Color(0xfff5f5f7); private val Text = Color(0xffe8e8ea)
private val Secondary = Color(0xffc7c7cc); private val Muted = Color(0xffa0a0a6); private val Faint = Color(0xff808087)
private val Accent = Color(0xff4c9aff); private val AccentStrong = Color(0xff3b78c7)
private val Success = Color(0xff5ad394); private val Danger = Color(0xffff8a80)
private val Palette = listOf("#4c9aff", "#ff9f43", "#3dd6c6", "#b48cff", "#ff6b8a", "#7ed957", "#f0b14c", "#8e8e93")
private val UiFont = FontFamily(Font(R.font.geist)); private val MonoFont = FontFamily(Font(R.font.geist_mono))
private fun profileColor(value: String?) = runCatching { Color(android.graphics.Color.parseColor(value ?: "#4c9aff")) }.getOrDefault(Accent)
private fun engineTitle(value: String) = when(value) { "postgres" -> "PostgreSQL"; "mysql" -> "MySQL"; else -> "Redis" }

@Composable fun DbmApp(vm: DbmViewModel = viewModel()) {
    val owner = LocalLifecycleOwner.current
    DisposableEffect(owner) {
        val observer = LifecycleEventObserver { _, event ->
            if (event == Lifecycle.Event.ON_STOP) vm.background()
            if (event == Lifecycle.Event.ON_START) vm.foreground()
        }
        owner.lifecycle.addObserver(observer); onDispose { owner.lifecycle.removeObserver(observer) }
    }
    val typography = Typography(
        bodyLarge = TextStyle(fontFamily = UiFont, fontSize = 13.sp),
        bodyMedium = TextStyle(fontFamily = UiFont, fontSize = 13.sp),
        bodySmall = TextStyle(fontFamily = UiFont, fontSize = 12.sp),
        labelLarge = TextStyle(fontFamily = UiFont, fontSize = 12.sp),
        titleMedium = TextStyle(fontFamily = UiFont, fontSize = 15.sp, fontWeight = FontWeight.SemiBold),
        headlineSmall = TextStyle(fontFamily = UiFont, fontSize = 17.sp, fontWeight = FontWeight.SemiBold),
    )
    MaterialTheme(colorScheme = darkColorScheme(background = Bg, surface = Chrome, primary = Accent, onPrimary = Color.White, onSurface = Text, error = Danger), typography = typography) {
        Surface(Modifier.fillMaxSize(), color = Bg) { Column(Modifier.safeDrawingPadding()) {
            if (vm.demo) Text("DEMO — local Rust fixture", Modifier.fillMaxWidth().background(Control).padding(horizontal = 12.dp, vertical = 4.dp), color = Muted, fontSize = 11.sp)
            if (vm.state.busy) LinearProgressIndicator(Modifier.fillMaxWidth().height(1.dp).testTag("loading"), color = Accent, trackColor = Border)
            if (vm.state.screen != Screen.Form) vm.state.error?.let { Feedback(it, Danger) }
            when (vm.state.screen) {
                Screen.Connections -> Connections(vm)
                Screen.Form -> Form(vm)
                Screen.Query, Screen.Explorer, Screen.Browse -> Workbench(vm)
            }
        } }
    }
}

@Composable private fun Connections(vm: DbmViewModel) {
    var deleting by remember { mutableStateOf<Profile?>(null) }
    Column(Modifier.fillMaxSize().background(Sidebar)) {
        Row(Modifier.fillMaxWidth().height(52.dp).padding(horizontal = 16.dp), verticalAlignment = Alignment.CenterVertically) {
            DbIcon(Icon.Database, Accent); Spacer(Modifier.width(9.dp)); Text("DBM", color = TextStrong, fontWeight = FontWeight.SemiBold, fontSize = 15.sp)
        }
        Text("Connections", color = Muted, fontWeight = FontWeight.SemiBold, fontSize = 11.sp, modifier = Modifier.padding(horizontal = 16.dp, vertical = 8.dp))
        LazyColumn(Modifier.weight(1f).padding(horizontal = 8.dp)) {
            if (!vm.state.busy && vm.state.profiles.isEmpty()) item { Text("No saved connections.", color = Muted, modifier = Modifier.padding(10.dp).testTag("empty")) }
            items(vm.state.profiles, key = { it.id }) { p ->
                Row(Modifier.fillMaxWidth().heightIn(min = 54.dp).clip(RoundedCornerShape(6.dp)).clickable(enabled = !vm.state.busy) { vm.select(p) }.padding(10.dp), verticalAlignment = Alignment.CenterVertically) {
                    Dot(profileColor(p.color), false); Spacer(Modifier.width(10.dp))
                    Column(Modifier.weight(1f)) { Text(p.name, color = Text, fontWeight = FontWeight.Medium); Text("${engineTitle(p.engine)}  ${p.host}:${p.port}", color = Faint, fontSize = 11.sp, maxLines = 1) }
                    SmallButton("Delete", danger = true, enabled = !vm.state.busy) { deleting = p }
                }
            }
        }
        Box(Modifier.fillMaxWidth().border(0.5.dp, Border).padding(10.dp)) { SecondaryButton("New connection", Icon.Plus, Modifier.fillMaxWidth().testTag("add"), !vm.state.busy, vm::add) }
    }
    deleting?.let { p -> AlertDialog(onDismissRequest = { deleting = null }, containerColor = Color(0xff262629), title = { Text("Delete ${p.name}?") }, text = { Text("This removes the saved connection and its local query history.", color = Secondary) }, confirmButton = { SmallButton("Delete connection", true) { deleting = null; vm.delete(p.id) } }, dismissButton = { SmallButton("Cancel") { deleting = null } }) }
}

@Composable private fun Form(vm: DbmViewModel) {
    val d = vm.state.draft
    LazyColumn(Modifier.fillMaxSize().background(Color(0xff262629)).padding(horizontal = 18.dp), verticalArrangement = Arrangement.spacedBy(12.dp), contentPadding = PaddingValues(vertical = 18.dp)) {
        item { Row(verticalAlignment = Alignment.Top) { Column(Modifier.weight(1f)) { Text(d.engine.uppercase(), color = Muted, fontSize = 11.sp, fontWeight = FontWeight.SemiBold); Text(if (d.id == null) "New connection" else "Edit connection", color = TextStrong, fontSize = 17.sp, fontWeight = FontWeight.SemiBold) }; SmallButton("Cancel", enabled = !vm.state.busy, action = vm::cancelForm) } }
        item { Label("Database engine"); Row(Modifier.fillMaxWidth().background(Control, RoundedCornerShape(7.dp)).padding(2.dp)) { listOf("postgres", "mysql", "redis").forEach { e -> Segment(engineTitle(e), d.engine == e, Modifier.weight(1f)) { vm.editDraft { it.copy(engine = e, port = when(e) { "postgres" -> "5432"; "mysql" -> "3306"; else -> "6379" }, database = if (e == "redis") "0" else "") } } } } }
        item { GField("Name", d.name) { vm.editDraft { x -> x.copy(name = it) } } }
        item { Label("Connection color"); Row(Modifier.horizontalScroll(rememberScrollState()), horizontalArrangement = Arrangement.spacedBy(8.dp)) { Palette.forEach { hex -> val selected = d.color.equals(hex, true); Box(Modifier.size(30.dp).clip(RoundedCornerShape(15.dp)).then(if(selected) Modifier.border(2.dp, Text, RoundedCornerShape(15.dp)) else Modifier).clickable { vm.editDraft { it.copy(color = hex) } }.testTag("color-$hex"), contentAlignment = Alignment.Center) { Dot(profileColor(hex), true) } }; GField("Custom", d.color, Modifier.width(112.dp)) { vm.editDraft { x -> x.copy(color = it) } } } }
        item { Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) { GField("Host", d.host, Modifier.weight(2f)) { vm.editDraft { x -> x.copy(host = it) } }; GField("Port", d.port, Modifier.weight(1f)) { vm.editDraft { x -> x.copy(port = it) } } } }
        item { Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) { GField(if(d.engine == "redis") "Username (optional)" else "User", d.username, Modifier.weight(1f)) { vm.editDraft { x -> x.copy(username = it) } }; GField(if(d.engine == "redis") "Database index" else "Database", d.database, Modifier.weight(1f)) { vm.editDraft { x -> x.copy(database = it) } } } }
        item { GField("Password", d.password, password = true, tag = "password") { vm.editDraft { x -> x.copy(password = it) } }; Text("Session only — cleared when DBM leaves the foreground.", color = Faint, fontSize = 11.sp, modifier = Modifier.padding(top = 5.dp)) }
        item { Row(horizontalArrangement = Arrangement.spacedBy(12.dp)) { Column(Modifier.weight(1f)) { Label("TLS"); ReadOnlyValue("Required") }; Column(Modifier.weight(1f)) { Label("Access"); ReadOnlyValue("Read-only") } } }
        vm.state.error?.let { item { Feedback(it, Danger) } }; vm.state.message?.let { item { Feedback(it, Success) } }
        item { Text("Passwords stay in memory and are never written to DBM's profile database.", color = Faint, fontSize = 11.sp) }
        item { Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(8.dp, Alignment.End)) { SecondaryButton("Test", enabled = !vm.state.busy, action = vm::test); SecondaryButton("Save", modifier = Modifier.testTag("save"), enabled = !vm.state.busy) { vm.save() }; PrimaryButton("Save & connect", enabled = !vm.state.busy) { vm.save(true) } } }
    }
}

@Composable private fun Workbench(vm: DbmViewModel) = BoxWithConstraints(Modifier.fillMaxSize()) {
    val wide = maxWidth >= 700.dp
    val drawer = rememberDrawerState(DrawerValue.Closed); val scope = rememberCoroutineScope()
    val content: @Composable () -> Unit = { WorkbenchMain(vm, if (wide) null else {{ scope.launch { drawer.open() }; Unit }}) }
    if (wide) Row { SourceList(vm, Modifier.width(250.dp)); Box(Modifier.weight(1f)) { content() } }
    else ModalNavigationDrawer(drawerState = drawer, drawerContent = { ModalDrawerSheet(drawerContainerColor = Sidebar, modifier = Modifier.width(286.dp)) { SourceList(vm, onNavigate = { scope.launch { drawer.close() } }) } }, content = content)
}

@Composable private fun SourceList(vm: DbmViewModel, modifier: Modifier = Modifier, onNavigate: () -> Unit = {}) {
    var filter by remember { mutableStateOf("") }; var databaseMenu by remember { mutableStateOf(false) }; val s = vm.state
    Column(modifier.fillMaxHeight().background(Sidebar).border(0.5.dp, Border)) {
        Row(Modifier.height(48.dp).fillMaxWidth().padding(horizontal = 12.dp), verticalAlignment = Alignment.CenterVertically) { DbIcon(Icon.Database, Accent); Spacer(Modifier.width(8.dp)); Text("DBM", color = TextStrong, fontWeight = FontWeight.SemiBold) }
        Column(Modifier.weight(1f).verticalScroll(rememberScrollState()).testTag("schema")) {
        Text("Connections", color = Muted, fontSize = 11.sp, fontWeight = FontWeight.SemiBold, modifier = Modifier.padding(horizontal = 12.dp, vertical = 8.dp))
        s.profiles.sortedBy { it.id != s.active?.id }.forEach { p ->
            Row(Modifier.fillMaxWidth().padding(horizontal = 8.dp).clip(RoundedCornerShape(6.dp)).background(if(s.active?.id == p.id) Color.White.copy(alpha = .06f) else Color.Transparent).clickable(enabled = !s.busy) { if(s.active?.id != p.id) vm.select(p); onNavigate() }.padding(10.dp).testTag("source-profile-${p.id}"), verticalAlignment = Alignment.CenterVertically) { Dot(profileColor(p.color), false); Spacer(Modifier.width(9.dp)); Column(Modifier.weight(1f)) { Text(p.name, color = Text, fontWeight = FontWeight.Medium, maxLines = 1); Text("${engineTitle(p.engine)}  ${p.host}:${p.port}", color = Faint, fontSize = 11.sp, maxLines = 1, overflow = TextOverflow.Ellipsis) } }
            if(s.active?.id == p.id) {
                Text("Database", color = Muted, fontSize = 11.sp, modifier = Modifier.padding(horizontal = 12.dp, vertical = 8.dp))
                Box(Modifier.padding(horizontal = 10.dp)) {
                    SecondaryButton(s.database, Icon.Database, Modifier.fillMaxWidth().testTag("database-picker"), !s.busy) { databaseMenu = true }
                    DropdownMenu(databaseMenu, { databaseMenu = false }) { s.databases.filter { it.isConnectable }.forEach { db -> DropdownMenuItem(text = { Text(db.name) }, onClick = { databaseMenu = false; vm.selectDatabase(db.name) }) } }
                }
                GField("Filter schema", filter, Modifier.padding(8.dp), tag = "tree-filter") { filter = it }
                Row(Modifier.fillMaxWidth().padding(horizontal = 12.dp, vertical = 5.dp), verticalAlignment = Alignment.CenterVertically) { Text("Schema", color = Muted, fontSize = 11.sp, fontWeight = FontWeight.SemiBold, modifier = Modifier.weight(1f)); SmallButton("Refresh", enabled = !s.busy, action = vm::refreshSchema) }
                SchemaTree(vm, s.tree, filter, onNavigate)
            }
        }
        }
        SecondaryButton("New connection", Icon.Plus, Modifier.fillMaxWidth().padding(10.dp), !s.busy, vm::add)
    }
}

private fun matches(node: SchemaNode, filter: String): Boolean = node.name.contains(filter, true) || node.children.any { matches(it, filter) }
@Composable private fun SchemaTree(vm: DbmViewModel, nodes: List<SchemaNode>, filter: String, onNavigate: () -> Unit, depth: Int = 0) {
    val s = vm.state
    nodes.filter { filter.isBlank() || matches(it, filter) }.forEach { node -> key(node.name, node.schema, node.table) {
        var expanded by remember { mutableStateOf(true) }
        Row(Modifier.padding(start = (depth * 14).dp)) {
            SourceRow(node.name, if(node.table != null) Icon.Table else if(expanded) Icon.ChevronDown else Icon.ChevronRight, s.table == (node.schema to node.table) && s.screen == Screen.Browse, profileColor(s.active?.color), enabled = !s.busy) {
                if(node.table != null) { vm.browse(node.schema ?: "", node.table); onNavigate() } else { expanded = !expanded }
            }
        }
        if(node.table == null && (expanded || filter.isNotBlank())) SchemaTree(vm, node.children, filter, onNavigate, depth + 1)
    } }
}

@Composable private fun WorkbenchMain(vm: DbmViewModel, openDrawer: (() -> Unit)?) {
    val s = vm.state; val color = profileColor(s.active?.color)
    Column(Modifier.fillMaxSize()) {
        Row(Modifier.fillMaxWidth().height(44.dp).background(Chrome).padding(horizontal = 10.dp), verticalAlignment = Alignment.CenterVertically) { if(openDrawer != null) { IconButton(openDrawer, Modifier.testTag("source-toggle").semantics { contentDescription = "Show source list" }) { DbIcon(Icon.Sidebar, Secondary) }; Spacer(Modifier.width(4.dp)) }; Dot(color, false); Spacer(Modifier.width(8.dp)); Column(Modifier.weight(1f)) { Text(s.active?.name ?: "Connection", color = TextStrong, fontWeight = FontWeight.Medium, maxLines = 1, overflow = TextOverflow.Ellipsis); Text(s.database, color = Muted, fontSize = 11.sp, maxLines = 1, overflow = TextOverflow.Ellipsis) }; IconButton(vm::disconnect, enabled = !s.busy, modifier = Modifier.semantics { contentDescription = "Disconnect" }) { DbIcon(Icon.Close, Secondary) } }
        Row(Modifier.fillMaxWidth().height(38.dp).background(Chrome).horizontalScroll(rememberScrollState())) {
            Tab("${if(s.active?.engine == "redis") "Redis" else "SQL"} query", Icon.Query, s.screen != Screen.Browse, color, vm::backToQuery)
            s.table?.let { table -> Tab(table.second, Icon.Table, s.screen == Screen.Browse, color, vm::showTable) }
        }
        when (s.screen) { Screen.Browse -> TablePane(vm); else -> QueryPane(vm) }
    }
}

@Composable private fun QueryPane(vm: DbmViewModel) { val s = vm.state
    Column(Modifier.fillMaxSize()) {
        Row(Modifier.fillMaxWidth().height(42.dp).background(Chrome).padding(horizontal = 10.dp), verticalAlignment = Alignment.CenterVertically) { Text(if(s.active?.engine == "redis") "Command" else "SQL editor", color = Secondary, fontSize = 12.sp, modifier = Modifier.weight(1f), maxLines = 1, overflow = TextOverflow.Ellipsis); SecondaryButton("Refresh", Icon.Refresh, Modifier.testTag("refresh-query"), !s.busy && s.lastExecuted != null, vm::refreshQuery); Spacer(Modifier.width(6.dp)); PrimaryButton("Run", Icon.Play, Modifier.testTag("run"), !s.busy && s.sql.isNotBlank(), vm::query) }
        GEditor(s.sql, vm::setSql, Modifier.fillMaxWidth().height(150.dp).testTag("editor"))
        Divider(); DataGrid(s.queryColumns, s.queryRows, Modifier.weight(1f))
        StatusFooter(if(s.queryColumns.isEmpty()) "Ready" else "${s.queryRows.size} rows", s.message)
    }
}

@Composable private fun TablePane(vm: DbmViewModel) { val s = vm.state
    Column(Modifier.fillMaxSize()) {
        Row(Modifier.fillMaxWidth().height(44.dp).background(Chrome).padding(horizontal = 10.dp), verticalAlignment = Alignment.CenterVertically) { DbIcon(Icon.Table, profileColor(s.active?.color)); Spacer(Modifier.width(7.dp)); Text(s.table?.let { "${it.first}.${it.second}" } ?: "Table", color = TextStrong, fontWeight = FontWeight.Medium, modifier = Modifier.weight(1f)); SecondaryButton("Refresh", Icon.Refresh, modifier = Modifier.testTag("refresh-table"), enabled = !s.busy) { s.table?.let { vm.browse(it.first, it.second, s.offset) } } }
        DataGrid(s.columns, s.rows, Modifier.weight(1f))
        Row(Modifier.fillMaxWidth().heightIn(min = 36.dp).background(Chrome).padding(horizontal = 10.dp), verticalAlignment = Alignment.CenterVertically) { Text(if(s.rows.isEmpty()) "0 rows" else "Rows ${s.offset + 1}–${s.offset + s.rows.size}", color = Muted, fontSize = 11.sp, modifier = Modifier.weight(1f)); SmallButton("Previous", enabled = s.offset > 0 && !s.busy) { s.table?.let { vm.browse(it.first, it.second, (s.offset - TABLE_PAGE_SIZE).coerceAtLeast(0)) } }; Spacer(Modifier.width(5.dp)); SmallButton("Next", enabled = s.hasMore && !s.busy, modifier = Modifier.testTag("next")) { s.table?.let { vm.browse(it.first, it.second, s.offset + TABLE_PAGE_SIZE) } } }
    }
}

@Composable private fun DataGrid(columns: List<String>, rows: List<List<JsonElement>>, modifier: Modifier = Modifier) {
    if(columns.isEmpty()) { Box(modifier.fillMaxWidth(), contentAlignment = Alignment.Center) { Text("No results", color = Faint, fontSize = 12.sp) }; return }
    Column(modifier.horizontalScroll(rememberScrollState()).testTag("results")) {
        Row(Modifier.background(GridHeader).testTag("results-header")) { columns.forEach { Cell(it, true) } }
        Column(Modifier.weight(1f).verticalScroll(rememberScrollState()).testTag("results-rows")) {
            rows.forEach { row -> Row { columns.indices.forEach { index -> val value = row.getOrNull(index); Cell(if(value == null || value is JsonNull) "NULL" else (value as? JsonPrimitive)?.contentOrNull ?: value.toString(), false) } } }
        }
    }
}
@Composable private fun Cell(value: String, header: Boolean) { Text(value, color = Text, fontFamily = MonoFont, fontSize = if(header) 11.sp else 12.sp, modifier = Modifier.width(160.dp).height(if(header) 34.dp else 32.dp).drawBehind { drawLine(if(header) Border else Color(0xff202023), Offset(0f, size.height), Offset(size.width, size.height), .5.dp.toPx()) }.padding(horizontal = 8.dp, vertical = 7.dp), maxLines = 1, overflow = TextOverflow.Ellipsis) }

@Composable private fun GField(label: String, value: String, modifier: Modifier = Modifier, password: Boolean = false, tag: String? = null, changed: (String) -> Unit) { Column(modifier) { Label(label); TextField(value, changed, singleLine = true, keyboardOptions = KeyboardOptions(autoCorrectEnabled = false, keyboardType = if(password) KeyboardType.Password else KeyboardType.Text), visualTransformation = if(password) PasswordVisualTransformation() else androidx.compose.ui.text.input.VisualTransformation.None, textStyle = TextStyle(Text, 13.sp, fontFamily = if(label == "Port" || label == "Custom") MonoFont else UiFont), colors = TextFieldDefaults.colors(focusedContainerColor = Control, unfocusedContainerColor = Control, focusedIndicatorColor = Accent, unfocusedIndicatorColor = BorderStrong, cursorColor = Accent), shape = RoundedCornerShape(6.dp), modifier = Modifier.fillMaxWidth().height(48.dp).then(if(tag != null) Modifier.testTag(tag) else Modifier)) } }
@Composable private fun GEditor(value: String, changed: (String) -> Unit, modifier: Modifier) { TextField(value, changed, keyboardOptions = KeyboardOptions(autoCorrectEnabled = false), textStyle = TextStyle(Text, 12.sp, fontFamily = MonoFont), colors = TextFieldDefaults.colors(focusedContainerColor = Bg, unfocusedContainerColor = Bg, focusedIndicatorColor = Accent, unfocusedIndicatorColor = Border, cursorColor = Accent), shape = RoundedCornerShape(0.dp), modifier = modifier) }
@Composable private fun Label(value: String) { Text(value, color = Muted, fontSize = 11.sp, fontWeight = FontWeight.Medium, modifier = Modifier.padding(bottom = 5.dp)) }
@Composable private fun ReadOnlyValue(value: String) { Text(value, Modifier.fillMaxWidth().background(Control, RoundedCornerShape(6.dp)).border(1.dp, BorderStrong, RoundedCornerShape(6.dp)).padding(11.dp), color = Text) }
@Composable private fun Feedback(value: String, color: Color) { Text(value, Modifier.fillMaxWidth().background(color.copy(alpha = .1f), RoundedCornerShape(7.dp)).border(1.dp, color.copy(alpha = .45f), RoundedCornerShape(7.dp)).padding(10.dp), color = color, fontSize = 12.sp) }
@Composable private fun Dot(color: Color, large: Boolean) { Canvas(Modifier.size(if(large) 20.dp else 9.dp)) { drawCircle(color) } }
@Composable private fun Divider() { HorizontalDivider(thickness = .5.dp, color = Border) }

@Composable private fun Segment(text: String, selected: Boolean, modifier: Modifier, action: () -> Unit) { Box(modifier.height(34.dp).clip(RoundedCornerShape(5.dp)).background(if(selected) ControlActive else Color.Transparent).clickable(onClick = action), contentAlignment = Alignment.Center) { Text(text, color = if(selected) TextStrong else Muted, fontSize = 12.sp, fontWeight = if(selected) FontWeight.Medium else FontWeight.Normal) } }
@Composable private fun Tab(text: String, icon: Icon, selected: Boolean, color: Color, action: () -> Unit) { Box(Modifier.widthIn(min = 132.dp).fillMaxHeight().testTag("tab-$text").clickable(onClick = action).background(if(selected) Bg else Chrome)) { Row(Modifier.fillMaxHeight().padding(horizontal = 12.dp), verticalAlignment = Alignment.CenterVertically) { DbIcon(icon, if(selected) color else Faint); Spacer(Modifier.width(7.dp)); Text(text, color = if(selected) TextStrong else Muted, fontSize = 12.sp, maxLines = 1) }; if(selected) Box(Modifier.matchParentSize().padding(bottom = 36.dp).background(color)) } }
@Composable private fun SourceRow(text: String, icon: Icon, selected: Boolean, color: Color, enabled: Boolean = true, action: () -> Unit) { Row(Modifier.fillMaxWidth().heightIn(min = 38.dp).padding(horizontal = 7.dp).clip(RoundedCornerShape(6.dp)).background(if(selected) color.copy(alpha = .18f) else Color.Transparent).clickable(enabled, onClick = action).padding(horizontal = 8.dp), verticalAlignment = Alignment.CenterVertically) { DbIcon(icon, if(selected) color else Faint); Spacer(Modifier.width(8.dp)); Text(text, color = if(selected) TextStrong else Secondary, fontSize = 12.sp, maxLines = 1, overflow = TextOverflow.Ellipsis) } }
@Composable private fun StatusFooter(left: String, right: String?) { Row(Modifier.fillMaxWidth().height(28.dp).background(Chrome).padding(horizontal = 10.dp), verticalAlignment = Alignment.CenterVertically) { Text(left, color = Faint, fontSize = 11.sp); Spacer(Modifier.weight(1f)); right?.let { Text(it, color = Muted, fontSize = 11.sp) } } }

@Composable private fun PrimaryButton(text: String, icon: Icon? = null, modifier: Modifier = Modifier, enabled: Boolean = true, action: () -> Unit) { Button(action, modifier.heightIn(min = 40.dp), enabled, shape = RoundedCornerShape(6.dp), colors = ButtonDefaults.buttonColors(AccentStrong, Color.White, Control, Faint), contentPadding = PaddingValues(horizontal = 13.dp, vertical = 7.dp)) { if(icon != null) { DbIcon(icon, Color.White); Spacer(Modifier.width(6.dp)) }; Text(text, fontSize = 12.sp, fontWeight = FontWeight.SemiBold) } }
@Composable private fun SecondaryButton(text: String, icon: Icon? = null, modifier: Modifier = Modifier, enabled: Boolean = true, action: () -> Unit) { Button(action, modifier.heightIn(min = 40.dp), enabled, shape = RoundedCornerShape(6.dp), colors = ButtonDefaults.buttonColors(Control, Text, Control, Faint), border = BorderStroke(1.dp, BorderStrong), contentPadding = PaddingValues(horizontal = 12.dp, vertical = 7.dp)) { if(icon != null) { DbIcon(icon, if(enabled) Secondary else Faint); Spacer(Modifier.width(6.dp)) }; Text(text, fontSize = 12.sp) } }
@Composable private fun SmallButton(text: String, danger: Boolean = false, enabled: Boolean = true, modifier: Modifier = Modifier, action: () -> Unit) { TextButton(action, modifier.heightIn(min = 40.dp), enabled, shape = RoundedCornerShape(5.dp), contentPadding = PaddingValues(horizontal = 9.dp, vertical = 5.dp), colors = ButtonDefaults.textButtonColors(contentColor = if(danger) Danger else Secondary, disabledContentColor = Faint)) { Text(text, fontSize = 12.sp) } }

private enum class Icon { Database, Table, Folder, Query, Refresh, Play, Plus, Close, Sidebar, ChevronDown, ChevronRight }
@Composable private fun DbIcon(icon: Icon, color: Color) { Canvas(Modifier.size(16.dp)) { withTransform({ scale(size.width / 16f, size.height / 16f, Offset.Zero) }) { val w = 1.5f; val stroke = Stroke(w, cap = StrokeCap.Round); when(icon) {
    Icon.Database -> { drawOval(color, topLeft = Offset(2f,2f), size = androidx.compose.ui.geometry.Size(12f,5f), style=stroke); drawLine(color,Offset(2f,4f),Offset(2f,12f),w); drawLine(color,Offset(14f,4f),Offset(14f,12f),w); drawArc(color,0f,180f,false,topLeft=Offset(2f,9f),size=androidx.compose.ui.geometry.Size(12f,5f),style=stroke) }
    Icon.Table -> { drawRect(color,Offset(2f,2f),androidx.compose.ui.geometry.Size(12f,12f),style=stroke); drawLine(color,Offset(2f,6f),Offset(14f,6f),w); drawLine(color,Offset(7f,2f),Offset(7f,14f),w) }
    Icon.Folder -> { val p=Path(); p.moveTo(2f,5f); p.lineTo(6f,5f); p.lineTo(8f,3f); p.lineTo(14f,3f); p.lineTo(14f,13f); p.lineTo(2f,13f); p.close(); drawPath(p,color,style=stroke) }
    Icon.Query -> { drawLine(color,Offset(3f,3f),Offset(13f,3f),w); drawLine(color,Offset(3f,7f),Offset(11f,7f),w); drawLine(color,Offset(3f,11f),Offset(9f,11f),w) }
    Icon.Refresh -> { drawArc(color,-50f,260f,false,topLeft=Offset(3f,3f),size=androidx.compose.ui.geometry.Size(10f,10f),style=stroke); drawLine(color,Offset(12f,2f),Offset(13f,6f),w) }
    Icon.Play -> { val p=Path(); p.moveTo(5f,3f); p.lineTo(13f,8f); p.lineTo(5f,13f); p.close(); drawPath(p,color,style=stroke) }
    Icon.Plus -> { drawLine(color,Offset(8f,3f),Offset(8f,13f),w); drawLine(color,Offset(3f,8f),Offset(13f,8f),w) }
    Icon.Close -> { drawLine(color,Offset(4f,4f),Offset(12f,12f),w); drawLine(color,Offset(12f,4f),Offset(4f,12f),w) }
    Icon.Sidebar -> { drawRect(color,Offset(2f,2f),androidx.compose.ui.geometry.Size(12f,12f),style=stroke); drawLine(color,Offset(6f,2f),Offset(6f,14f),w) }
    Icon.ChevronDown -> { drawLine(color,Offset(4f,6f),Offset(8f,10f),w); drawLine(color,Offset(8f,10f),Offset(12f,6f),w) }
    Icon.ChevronRight -> { drawLine(color,Offset(6f,4f),Offset(10f,8f),w); drawLine(color,Offset(10f,8f),Offset(6f,12f),w) }
} } } }
