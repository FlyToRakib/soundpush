package net.soundpush.settings

import org.junit.Assert.assertEquals
import org.junit.Test

class BatteryGuidesTest {
    @Test
    fun detectsPhoneMakers() {
        assertEquals(PhoneMaker.Xiaomi, BatteryGuides.detect("Xiaomi", "Redmi"))
        assertEquals(PhoneMaker.Xiaomi, BatteryGuides.detect("Xiaomi", "POCO"))
        assertEquals(PhoneMaker.Samsung, BatteryGuides.detect("samsung", "samsung"))
        assertEquals(PhoneMaker.Huawei, BatteryGuides.detect("HONOR", "HONOR"))
        assertEquals(PhoneMaker.OnePlus, BatteryGuides.detect("OnePlus", "OnePlus"))
        assertEquals(PhoneMaker.Oppo, BatteryGuides.detect("realme", "realme"))
        assertEquals(PhoneMaker.Oppo, BatteryGuides.detect("OPPO", "OPPO"))
        assertEquals(PhoneMaker.Generic, BatteryGuides.detect("Google", "google"))
    }

    @Test
    fun everyMakerHasStepsAndAnAppSettingsFallback() {
        PhoneMaker.entries.forEach { maker ->
            assert(BatteryGuides.steps(maker).isNotEmpty()) { "$maker has no steps" }
            assert(BatteryGuides.links(maker).any { it.label == net.soundpush.ui.R.string.battery_open_app }) { "$maker has no fallback" }
        }
    }
}
