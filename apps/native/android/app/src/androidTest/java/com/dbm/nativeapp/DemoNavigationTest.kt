package com.dbm.nativeapp

import android.app.UiAutomation
import android.content.Intent
import android.graphics.Bitmap
import android.os.ParcelFileDescriptor
import android.text.InputType
import androidx.compose.ui.graphics.asAndroidBitmap
import androidx.compose.ui.test.*
import androidx.compose.ui.test.junit4.createEmptyComposeRule
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.ViewModelProvider
import androidx.test.core.app.ApplicationProvider
import androidx.test.ext.junit.rules.ActivityScenarioRule
import androidx.test.platform.app.InstrumentationRegistry
import java.io.File
import org.junit.Assert.assertEquals
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
        compose.onNodeWithTag("add").performClick()
        compose.onNodeWithText("New connection").assertExists()
        capture("connection-form")
        compose.onNodeWithText("Test").performScrollTo().performClick()
        val validationError = "invalid input: name, host, and username are required"
        compose.waitUntil(10_000) { compose.onAllNodesWithText(validationError).fetchSemanticsNodes().isNotEmpty() }
        compose.onNodeWithText(validationError).performScrollTo().assertIsDisplayed()
        capture("connection-error")
        compose.onNodeWithText("Cancel").performScrollTo().performClick()
        compose.onNodeWithText("Production").performClick()
        compose.waitUntil(10_000) { compose.onAllNodesWithTag("editor").fetchSemanticsNodes().isNotEmpty() }
        compose.onNodeWithTag("run").performClick()
        compose.waitUntil(10_000) { compose.onAllNodesWithTag("results").fetchSemanticsNodes().isNotEmpty() }
        compose.onNodeWithText("Maya Okafor").assertExists()
        capture("query")
        compose.onNodeWithTag("source-toggle").performClick()
        capture("source-list")
        compose.onNodeWithText("users").performScrollTo().performClick()
        compose.waitUntil(10_000) { compose.onAllNodesWithText("Rows 1–25").fetchSemanticsNodes().isNotEmpty() }
        compose.onNodeWithTag("editor").assertDoesNotExist()
        compose.onNodeWithText("email").assertExists()
        compose.onNodeWithText("Previous").assertIsNotEnabled()
        compose.onNodeWithTag("next").assertIsEnabled().performClick()
        compose.waitUntil(10_000) { compose.onAllNodesWithText("Rows 26–40").fetchSemanticsNodes().isNotEmpty() }
        compose.onNodeWithTag("next").assertIsNotEnabled()
        compose.onNodeWithTag("refresh-table").performClick()
        compose.onNodeWithText("Rows 26–40").assertExists()
        compose.onNodeWithText("Previous").performClick()
        compose.waitUntil(10_000) { compose.onAllNodesWithText("Rows 1–25").fetchSemanticsNodes().isNotEmpty() }
        capture("table")
        val headerTop = compose.onNodeWithTag("results-header").fetchSemanticsNode().boundsInRoot.top
        val firstRowTop = compose.onNodeWithText("Maya Okafor").fetchSemanticsNode().boundsInRoot.top
        compose.onNodeWithTag("results-rows").performTouchInput { swipeUp() }
        compose.onNodeWithText("id").assertIsDisplayed()
        assertEquals(headerTop, compose.onNodeWithTag("results-header").fetchSemanticsNode().boundsInRoot.top, 0.5f)
        check(compose.onNodeWithText("Maya Okafor").fetchSemanticsNode().boundsInRoot.top < firstRowTop)
        capture("table-scrolled")
        compose.onNodeWithText("SQL query").performClick()
        compose.onNodeWithTag("editor").assertTextEquals("SELECT 1")
        compose.onNodeWithText("Maya Okafor").assertExists()
        compose.onNodeWithText("paid_total").assertExists()
        compose.onNodeWithText("email").assertDoesNotExist()
        compose.onNodeWithTag("source-toggle").performClick()
        compose.onNodeWithText("New connection").performClick()
        compose.onNodeWithText("Cancel").performClick()
        compose.onNodeWithTag("editor").assertTextEquals("SELECT 1")
        compose.onNodeWithText("paid_total").assertExists()
        compose.onNodeWithTag("tab-users").performClick()
        compose.onNodeWithText("Rows 1–25").assertExists()

        val automation = InstrumentationRegistry.getInstrumentation().uiAutomation
        try {
            check(automation.setRotation(UiAutomation.ROTATION_FREEZE_90))
            compose.waitUntil(10_000) { compose.onAllNodesWithTag("source-toggle").fetchSemanticsNodes().isEmpty() }
            compose.onNodeWithTag("tree-filter").assertExists()
            compose.onNodeWithTag("database-picker").assertIsDisplayed()
            compose.onNodeWithText("Rows 1–25").assertExists()
            compose.onNodeWithTag("editor").assertDoesNotExist()
            compose.onNode(hasText("users") and hasAnyAncestor(hasTestTag("schema"))).performScrollTo().assertIsDisplayed()
            capture("landscape-table")
        } finally { automation.setRotation(UiAutomation.ROTATION_UNFREEZE) }

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
        compose.onNodeWithContentDescription("Disconnect").performClick()
        awaitConnections()
        compose.onNodeWithTag("editor").assertDoesNotExist()
    }

    @Test fun queryRefreshRerunsExecutedSqlWithoutReplacingDraft() {
        awaitConnections()
        compose.onNodeWithText("Production").performClick()
        compose.waitUntil(10_000) { compose.onAllNodesWithTag("editor").fetchSemanticsNodes().isNotEmpty() }
        compose.onNodeWithTag("refresh-query").assertIsNotEnabled()
        val runTop = compose.onNodeWithTag("run").fetchSemanticsNode().boundsInRoot.top
        check(runTop < compose.onNodeWithTag("editor").fetchSemanticsNode().boundsInRoot.top)
        compose.onNodeWithTag("run").performClick()
        compose.waitUntil(10_000) { compose.onAllNodes(hasTestTag("refresh-query") and isEnabled()).fetchSemanticsNodes().isNotEmpty() }
        compose.onNodeWithTag("editor").performTextReplacement("SHOW DATABASES")
        compose.onNodeWithTag("refresh-query").performClick()
        compose.waitUntil(10_000) { compose.onAllNodes(hasTestTag("refresh-query") and isEnabled()).fetchSemanticsNodes().isNotEmpty() }
        compose.onNodeWithTag("editor").assertTextEquals("SHOW DATABASES")
        compose.onNodeWithText("Maya Okafor").assertExists()
        compose.onNodeWithText("DBM demo").assertDoesNotExist()
        capture("query-refresh")
        compose.onNodeWithTag("run").performClick()
        compose.waitUntil(10_000) { compose.onAllNodesWithText("DBM demo").fetchSemanticsNodes().isNotEmpty() }
        compose.onNodeWithText("Maya Okafor").assertDoesNotExist()
        compose.onNodeWithTag("editor").performTextReplacement("SELECT 1")
        compose.onNodeWithTag("refresh-query").performClick()
        compose.waitUntil(10_000) { compose.onAllNodes(hasTestTag("refresh-query") and isEnabled()).fetchSemanticsNodes().isNotEmpty() }
        compose.onNodeWithText("DBM demo").assertExists()
        compose.onNodeWithTag("editor").assertTextEquals("SELECT 1")
    }

    @Test fun passwordKeyboardUsesPasswordTypeAndSqlDoesNotAutocorrect() {
        awaitConnections()
        compose.onNodeWithTag("add").performClick()
        compose.onNodeWithTag("password").performScrollTo().performClick().performTextInput("memory-only")
        activity.scenario.onActivity { assertEquals("memory-only", ViewModelProvider(it)[DbmViewModel::class.java].state.draft.password) }
        compose.waitUntil(10_000) {
            currentInputType()?.let { it and InputType.TYPE_MASK_VARIATION == InputType.TYPE_TEXT_VARIATION_PASSWORD } == true
        }
        val passwordType = requireNotNull(currentInputType())
        assertEquals(InputType.TYPE_CLASS_TEXT, passwordType and InputType.TYPE_MASK_CLASS)
        assertEquals(InputType.TYPE_TEXT_VARIATION_PASSWORD, passwordType and InputType.TYPE_MASK_VARIATION)
        assertEquals(0, passwordType and InputType.TYPE_TEXT_FLAG_AUTO_CORRECT)
        capture("password-input")
        compose.onNodeWithText("Cancel").performScrollTo().performClick()
        compose.onNodeWithText("Production").performClick()
        compose.waitUntil(10_000) { compose.onAllNodesWithTag("editor").fetchSemanticsNodes().isNotEmpty() }
        compose.onNodeWithTag("editor").performClick()
        compose.waitUntil(10_000) {
            currentInputType()?.let { it and InputType.TYPE_TEXT_FLAG_MULTI_LINE != 0 } == true
        }
        val sqlType = requireNotNull(currentInputType())
        assertEquals(InputType.TYPE_CLASS_TEXT, sqlType and InputType.TYPE_MASK_CLASS)
        assertEquals(InputType.TYPE_TEXT_VARIATION_NORMAL, sqlType and InputType.TYPE_MASK_VARIATION)
        assertEquals(0, sqlType and InputType.TYPE_TEXT_FLAG_AUTO_CORRECT)
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

    private fun currentInputType(): Int? {
        val output = InstrumentationRegistry.getInstrumentation().uiAutomation.executeShellCommand("dumpsys input_method")
        val dump = ParcelFileDescriptor.AutoCloseInputStream(output).bufferedReader().use { it.readText() }
        return Regex("inputType=0x([0-9a-fA-F]+)").find(dump)?.groupValues?.get(1)?.toIntOrNull(16)
    }

    private fun capture(name: String) {
        val directory = File(requireNotNull(InstrumentationRegistry.getArguments().getString("additionalTestOutputDir")))
        check(directory.mkdirs() || directory.isDirectory)
        val image = compose.onRoot().captureToImage().asAndroidBitmap()
        try { File(directory, "$name.png").outputStream().use { check(image.compress(Bitmap.CompressFormat.PNG, 100, it)) } }
        finally { image.recycle() }
    }
}
