package net.soundpush.benchmark

import androidx.benchmark.macro.FrameTimingMetric
import androidx.benchmark.macro.StartupMode
import androidx.benchmark.macro.StartupTimingMetric
import androidx.benchmark.macro.junit4.MacrobenchmarkRule
import androidx.test.ext.junit.runners.AndroidJUnit4
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith

/**
 * How long SoundPush takes to draw its first frame, and how smoothly Home scrolls afterwards
 * (plan §29.1 "Android UI", §22.1 startup budget).
 *
 * Cold start is the honest number: the process, the native engine library and the Compose runtime
 * all load. The engine itself starts on a background thread, so the first frame is the loading state
 * — which is exactly what this measures, and why the app does it that way.
 *
 * Run with `./gradlew :benchmark:connectedBenchmarkAndroidTest` against a device or emulator.
 */
@RunWith(AndroidJUnit4::class)
class StartupBenchmark {
    @get:Rule
    val benchmarkRule = MacrobenchmarkRule()

    @Test
    fun coldStartup() = benchmarkRule.measureRepeated(
        packageName = TARGET,
        metrics = listOf(StartupTimingMetric()),
        iterations = ITERATIONS,
        startupMode = StartupMode.COLD,
        setupBlock = { pressHome() },
    ) {
        startActivityAndWait()
    }

    /**
     * A warm start, where the process survives but the activity does not: the path a user takes
     * coming back from the notification or the Quick Settings tile.
     */
    @Test
    fun warmStartup() = benchmarkRule.measureRepeated(
        packageName = TARGET,
        metrics = listOf(StartupTimingMetric(), FrameTimingMetric()),
        iterations = ITERATIONS,
        startupMode = StartupMode.WARM,
        setupBlock = { pressHome() },
    ) {
        startActivityAndWait()
    }

    private companion object {
        const val TARGET = "net.soundpush.android"

        /** Enough repetitions for a stable median without making a CI run long. */
        const val ITERATIONS = 10
    }
}
