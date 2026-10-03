package com.dbm.nativeapp

import kotlinx.serialization.Serializable
import kotlinx.serialization.json.JsonElement

const val TABLE_PAGE_SIZE = 25

@Serializable data class Profile(val id: String, val name: String, val color: String? = null, val engine: String,
    val host: String, val port: Int, val username: String, val defaultDatabase: String, val tlsMode: String,
    val caCertPath: String? = null, val readOnly: Boolean)
@Serializable data class ProfileSummary(val profile: Profile)
@Serializable data class DatabaseRef(val name: String, val isTemplate: Boolean = false, val isConnectable: Boolean = true)
@Serializable data class Workspace(val profile: Profile, val databases: List<DatabaseRef>)
@Serializable data class SchemaNode(val name: String, val kind: String, val schema: String? = null,
    val table: String? = null, val children: List<SchemaNode> = emptyList())
@Serializable data class QueryColumn(val name: String)
@Serializable data class QueryResult(val columns: List<QueryColumn> = emptyList(), val rows: List<List<JsonElement>> = emptyList(),
    val truncated: Boolean = false, val durationMs: Long = 0)
@Serializable data class TablePage(val columns: List<String>, val rows: List<List<JsonElement>>, val offset: Int,
    val limit: Int, val hasMore: Boolean)

data class ProfileDraft(val id: String? = null, val name: String = "", val color: String = "#4c9aff",
    val engine: String = "postgres", val host: String = "", val port: String = "5432", val username: String = "",
    val database: String = "", val password: String = "")

enum class Screen { Connections, Form, Query, Explorer, Browse }
data class UiState(val screen: Screen = Screen.Connections, val profiles: List<Profile> = emptyList(),
    val draft: ProfileDraft = ProfileDraft(), val active: Profile? = null, val databases: List<DatabaseRef> = emptyList(),
    val database: String = "", val tree: List<SchemaNode> = emptyList(), val sql: String = "SELECT 1",
    val queryColumns: List<String> = emptyList(), val queryRows: List<List<JsonElement>> = emptyList(),
    val columns: List<String> = emptyList(), val rows: List<List<JsonElement>> = emptyList(),
    val table: Pair<String, String>? = null, val offset: Int = 0, val hasMore: Boolean = false,
    val busy: Boolean = false, val error: String? = null, val message: String? = null)
