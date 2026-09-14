package net.soundpush.app

import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Modifier
import com.mikepenz.aboutlibraries.ui.compose.android.rememberLibraries
import com.mikepenz.aboutlibraries.ui.compose.m3.LibrariesContainer
import net.soundpush.ui.components.readableWidth

/**
 * Settings → About → Open-source licences. The list is generated at build time by the
 * AboutLibraries Gradle plugin from the app's resolved dependencies and their POM licence data
 * (offline, so the same sources give the same list). The engine's Rust crates are covered by
 * `cargo deny` licence checks in CI and listed in the repository.
 */
@Composable
internal fun LicensesScreen() {
    val libraries by rememberLibraries(R.raw.aboutlibraries)
    LibrariesContainer(libraries, Modifier.readableWidth())
}
