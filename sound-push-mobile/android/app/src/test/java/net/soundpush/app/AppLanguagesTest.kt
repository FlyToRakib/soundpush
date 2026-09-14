package net.soundpush.app

import org.junit.Assert.assertEquals
import org.junit.Test

class AppLanguagesTest {
    @Test
    fun languagesAreNamedInTheirOwnLanguage() {
        assertEquals("English", AppLanguages.displayName("en"))
        assertEquals("Deutsch", AppLanguages.displayName("de"))
        assertEquals("Français", AppLanguages.displayName("fr"))
    }
}
