package com.dbm.nativeapp

import android.content.Intent
import android.graphics.Bitmap
import androidx.compose.ui.graphics.asAndroidBitmap
import androidx.compose.ui.test.*
import androidx.compose.ui.test.junit4.createEmptyComposeRule
import androidx.lifecycle.Lifecycle
import androidx.test.core.app.ApplicationProvider
import androidx.test.ext.junit.rules.ActivityScenarioRule
import androidx.test.platform.app.InstrumentationRegistry
import java.io.File
import org.junit.Rule
import org.junit.rules.RuleChain
import org.junit.Test

class DemoNavigationTest {
    private val intent = Intent(ApplicationProvider.getApplicationContext(), MainActivity::class.java).putExtra("demo", true)
    private val activity = ActivityScenarioRule<MainActivity>(intent)
    private val compose = createEmptyComposeRule()
    @get:Rule val rules: RuleChain = RuleChain.outerRule(compose).around(activity)

    @Test fun demoNavigatesQueryAndBrowseScreens() {
        compose.waitUntil(10_000) { compose.onAllNodesWithText("Production").fetchSemanticsNodes().isNotEmpty() }
        capture("connections")
        compose.onNodeWithText("Production").performClick()
        compose.waitUntil(10_000) { compose.onAllNodesWithTag("editor").fetchSemanticsNodes().isNotEmpty() }
        compose.onNodeWithTag("run").performClick()
        compose.waitUntil(10_000) { compose.onAllNodesWithTag("results").fetchSemanticsNodes().isNotEmpty() }
        compose.onNodeWithText("Maya Okafor").assertExists()
        capture("query")
        compose.onNodeWithText("Explorer").performClick()
        compose.onNodeWithText("users").performClick()
        compose.waitUntil(10_000) { compose.onAllNodesWithText("Rows 1–25").fetchSemanticsNodes().isNotEmpty() }
        compose.onNodeWithText("Previous").assertIsNotEnabled()
        compose.onNodeWithTag("next").assertIsEnabled().performClick()
        compose.waitUntil(10_000) { compose.onAllNodesWithText("Rows 26–40").fetchSemanticsNodes().isNotEmpty() }
        compose.onNodeWithTag("next").assertIsNotEnabled()
        compose.onNodeWithText("Refresh").performClick()
        compose.onNodeWithText("Rows 26–40").assertExists()
        compose.onNodeWithText("Previous").performClick()
        compose.waitUntil(10_000) { compose.onAllNodesWithText("Rows 1–25").fetchSemanticsNodes().isNotEmpty() }
        capture("table")

        activity.scenario.moveToState(Lifecycle.State.CREATED)
        activity.scenario.moveToState(Lifecycle.State.RESUMED)
        awaitConnections()
        compose.onNodeWithTag("editor").assertDoesNotExist()
        compose.onNodeWithTag("results").assertDoesNotExist()
        compose.onNodeWithText("Production").performClick()
        compose.waitUntil(10_000) { compose.onAllNodesWithTag("editor").fetchSemanticsNodes().isNotEmpty() }
        activity.scenario.recreate()
        awaitConnections()
        compose.onNodeWithTag("editor").assertDoesNotExist()
        compose.onNodeWithText("Production").performClick()
        compose.waitUntil(10_000) { compose.onAllNodesWithTag("editor").fetchSemanticsNodes().isNotEmpty() }
        compose.onNodeWithTag("run").assertIsEnabled()
    }

    @Test fun deletingAConnectionRequiresConfirmation() {
        awaitConnections()
        compose.onAllNodesWithText("Delete")[0].performClick()
        compose.onNodeWithText("Delete Production?").assertExists()
        compose.onNodeWithText("Cancel").performClick()
        compose.onNodeWithText("Production").assertExists()
        compose.onAllNodesWithText("Delete")[0].performClick()
        compose.onNodeWithText("Delete connection").performClick()
        compose.waitUntil(10_000) { compose.onAllNodesWithText("Production").fetchSemanticsNodes().isEmpty() }
    }

    private fun awaitConnections() {
        compose.waitUntil(10_000) {
            compose.onAllNodes(hasTestTag("add") and isEnabled()).fetchSemanticsNodes().isNotEmpty() &&
                compose.onAllNodesWithText("Production").fetchSemanticsNodes().isNotEmpty()
        }
    }

    private fun capture(name: String) {
        val directory = File(requireNotNull(InstrumentationRegistry.getArguments().getString("additionalTestOutputDir")))
        check(directory.mkdirs() || directory.isDirectory)
        val image = compose.onRoot().captureToImage().asAndroidBitmap()
        try { File(directory, "$name.png").outputStream().use { check(image.compress(Bitmap.CompressFormat.PNG, 100, it)) } }
        finally { image.recycle() }
    }
}
