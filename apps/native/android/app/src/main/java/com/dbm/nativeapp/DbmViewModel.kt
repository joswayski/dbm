package com.dbm.nativeapp

import android.app.Application
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.*
import kotlinx.serialization.json.*

class DbmViewModel(application: Application, val demo: Boolean = false) : AndroidViewModel(application) {
    private val bridge = BridgeClient(application)
    var state by mutableStateOf(UiState()); private set
    private var epoch = 0L
    private var foreground = true
    private var closing = false
    private var formReturnScreen = Screen.Connections

    init { start() }
    private fun start() = launch { expected -> bridge.open(demo); profiles(expected) }
    private fun launch(block: suspend (Long) -> Unit) {
        if (state.busy || !foreground) return
        val expected = epoch
        state = state.copy(busy = true, error = null, message = null)
        viewModelScope.launch {
            try { block(expected) }
            catch (e: CancellationException) { throw e }
            catch (e: Exception) { if (expected == epoch) state = state.copy(error = e.message ?: "Operation failed") }
            finally { if (expected == epoch) state = state.copy(busy = false) }
        }
    }
    private suspend fun call(expected: Long, request: JsonObject): JsonElement {
        checkEpoch(expected)
        val value = bridge.call(request)
        checkEpoch(expected)
        return value
    }
    private fun checkEpoch(expected: Long) { if (expected != epoch) throw CancellationException("Session changed") }
    private suspend fun profiles(expected: Long) {
        val values: List<ProfileSummary> = bridge.json.decodeFromJsonElement(call(expected, command("listProfiles")))
        state = state.copy(profiles = values.map { it.profile })
    }
    fun add() { formReturnScreen = state.screen; state = state.copy(screen = Screen.Form, draft = ProfileDraft()) }
    fun select(profile: Profile) {
        if (demo) connect(profile)
        else { formReturnScreen = state.screen; state = state.copy(screen = Screen.Form, draft = ProfileDraft(id = profile.id, name = profile.name,
            color = profile.color ?: "#4c9aff", engine = profile.engine, host = profile.host,
            port = profile.port.toString(), username = profile.username, database = profile.defaultDatabase)) }
    }
    fun cancelForm() { state = state.copy(screen = formReturnScreen, draft = ProfileDraft(), error = null, message = null) }
    fun editDraft(transform: (ProfileDraft) -> ProfileDraft) { if (!state.busy) state = state.copy(draft = transform(state.draft)) }
    private suspend fun input(): JsonObject {
        val draft = state.draft
        val roots = withContext(Dispatchers.IO) { bridge.writePlatformCaBundle() }
        return buildJsonObject {
            draft.id?.let { put("id", it) }; put("name", draft.name); put("color", draft.color)
            put("engine", draft.engine); put("host", draft.host); put("port", draft.port.toIntOrNull() ?: 0)
            put("username", draft.username); put("defaultDatabase", draft.database); put("password", draft.password)
            put("tlsMode", "required"); put("caCertPath", roots); put("readOnly", true)
        }
    }
    fun test() = launch { expected -> call(expected, buildJsonObject { put("command", "testProfile"); put("input", input()) }); state = state.copy(message = "Connection succeeded") }
    fun save(connectAfter: Boolean = false) = launch { expected ->
        val profile: Profile = bridge.json.decodeFromJsonElement(call(expected, buildJsonObject { put("command", "saveProfile"); put("input", input()) }))
        state = state.copy(screen = formReturnScreen, draft = ProfileDraft())
        profiles(expected)
        if (connectAfter) connect(expected, profile)
    }
    fun delete(id: String) = launch { expected -> call(expected, command("deleteProfile", id)); profiles(expected) }
    fun connect(profile: Profile) = launch { expected -> connect(expected, profile) }
    private suspend fun connect(expected: Long, profile: Profile) {
        val workspace: Workspace = bridge.json.decodeFromJsonElement(call(expected, command("connect", profile.id)))
        state = state.copy(screen = Screen.Query, active = workspace.profile, databases = workspace.databases,
            database = workspace.profile.defaultDatabase, sql = if (profile.engine == "redis") "PING" else "SELECT 1",
            queryColumns = emptyList(), queryRows = emptyList(), columns = emptyList(), rows = emptyList(), table = null)
        loadSchema(expected, profile.id)
    }
    private suspend fun loadSchema(expected: Long, id: String) {
        val tree: List<SchemaNode> = bridge.json.decodeFromJsonElement(call(expected, command("loadSchemaTree", id)))
        state = state.copy(tree = tree)
    }
    fun refreshSchema() = launch { expected -> state.active?.let { loadSchema(expected, it.id) } }
    fun explorer() { state = state.copy(screen = Screen.Explorer) }
    fun selectDatabase(database: String) = launch { expected ->
        val p = state.active ?: return@launch
        call(expected, buildJsonObject { put("command", "connectDatabase"); put("profile_id", p.id); put("database", database) })
        state = state.copy(screen = Screen.Query, database = database, queryColumns = emptyList(), queryRows = emptyList(),
            columns = emptyList(), rows = emptyList(), table = null)
        loadSchema(expected, p.id)
    }
    fun setSql(sql: String) { if (!state.busy) state = state.copy(sql = sql) }
    fun query() = launch { expected ->
        val p = state.active ?: return@launch
        val result: QueryResult = bridge.json.decodeFromJsonElement(call(expected, buildJsonObject { put("command", "query"); putJsonObject("request") { put("profileId", p.id); put("sql", state.sql); put("maxRows", 1000) } }))
        state = state.copy(queryColumns = result.columns.map { it.name }, queryRows = result.rows,
            message = if (result.truncated) "Results truncated at 1,000 rows" else "${result.rows.size} rows · ${result.durationMs} ms")
    }
    fun browse(schema: String, table: String, offset: Int = 0) = launch { expected ->
        val p = state.active ?: return@launch
        val page: TablePage = bridge.json.decodeFromJsonElement(call(expected, buildJsonObject { put("command", "loadTablePage"); putJsonObject("request") { put("profileId", p.id); put("schema", schema); put("table", table); put("offset", offset); put("limit", TABLE_PAGE_SIZE); putJsonArray("filters") {}; put("orderBy", JsonNull); put("includeTotal", false) } }))
        state = state.copy(screen = Screen.Browse, table = schema to table, columns = page.columns, rows = page.rows, offset = page.offset, hasMore = page.hasMore)
    }
    fun backToQuery() { state = state.copy(screen = Screen.Query) }
    fun showTable() { if (state.table != null) state = state.copy(screen = Screen.Browse) }
    fun disconnect() { background(); foreground() }
    fun background() {
        foreground = false
        epoch++; state = UiState(busy = true)
        formReturnScreen = Screen.Connections
        closing = true
        viewModelScope.launch {
            bridge.close()
            closing = false; state = UiState()
            if (foreground) start()
        }
    }
    fun foreground() { foreground = true; if (!closing) start() }
    override fun onCleared() { epoch++; CoroutineScope(Dispatchers.IO).launch { bridge.close() } }
    private fun command(name: String, id: String? = null) = buildJsonObject { put("command", name); id?.let { put("profile_id", it) } }
}
