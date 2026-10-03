package com.dbm.nativeapp

import kotlinx.serialization.json.Json
import org.junit.Assert.assertEquals
import org.junit.Test

class ModelsTest {
    @Test fun queryColumnsAreObjectsAndTableColumnsAreStrings() {
        val query = Json.decodeFromString<QueryResult>("""{"columns":[{"name":"answer"}],"rows":[[42]]}""")
        val table = Json.decodeFromString<TablePage>("""{"columns":["id"],"rows":[[1]],"offset":0,"limit":50,"hasMore":false}""")
        assertEquals("answer", query.columns.single().name)
        assertEquals("id", table.columns.single())
    }

    @Test fun mobileDraftDefaultsToPostgresTlsPort() { assertEquals("5432", ProfileDraft().port) }
}
