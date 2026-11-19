package dev.dirt.capture

import android.content.Context
import android.content.res.Configuration
import java.io.File

// The theme and the accent the person chose in the app (Settings →
// Appearance), for the parts drawn here and not in the web view. The app
// writes them to a small file whenever they change (`capture::write_look`
// in dirt-mobile): "<theme> <accent>", such as "dark moss". It says nothing
// about any note.
object Look {
    private fun file(context: Context) = File(Waiting.dataFolder(context), "look")

    private fun read(context: Context): List<String> =
        runCatching { file(context).readText().trim().split(' ') }.getOrDefault(emptyList())

    // "light", "dark", or "auto" to follow the system.
    fun theme(context: Context): String = read(context).getOrNull(0) ?: "auto"

    // One of the accents' names; Clay when none was chosen.
    fun accent(context: Context): String = read(context).getOrNull(1) ?: "clay"

    // `base`, in the theme the person chose: the sheet then takes its
    // colours from `values` or `values-night` as the app does.
    fun themed(base: Context): Context {
        val night = when (theme(base)) {
            "light" -> Configuration.UI_MODE_NIGHT_NO
            "dark" -> Configuration.UI_MODE_NIGHT_YES
            else -> return base
        }
        val config = Configuration(base.resources.configuration)
        config.uiMode = (config.uiMode and Configuration.UI_MODE_NIGHT_MASK.inv()) or night
        return base.createConfigurationContext(config)
    }

    // The accent's colour resource, and the colour of what is drawn on it.
    fun accentColour(name: String): Int = when (name) {
        "moss" -> R.color.dirt_accent_moss
        "lake" -> R.color.dirt_accent_lake
        "plum" -> R.color.dirt_accent_plum
        "honey" -> R.color.dirt_accent_honey
        "graphite" -> R.color.dirt_accent_graphite
        else -> R.color.dirt_accent_clay
    }

    fun onAccentColour(name: String): Int = when (name) {
        "moss" -> R.color.dirt_on_accent_moss
        "lake" -> R.color.dirt_on_accent_lake
        "plum" -> R.color.dirt_on_accent_plum
        "honey" -> R.color.dirt_on_accent_honey
        "graphite" -> R.color.dirt_on_accent_graphite
        else -> R.color.dirt_on_accent_clay
    }
}
