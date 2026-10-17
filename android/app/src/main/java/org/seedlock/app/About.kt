package org.seedlock.app

import android.view.View
import android.widget.TextView
import org.seedlock.ffi.seedlockVersion
import java.io.IOException

/** Where SeedLock's source code is. */
const val SOURCE_CODE = "https://github.com/Solo-in-Code/SeedLock"

/**
 * What SeedLock is, its license, and the license notices of the libraries it is built from. The notices
 * are the app's file assets/notices.txt, which android/build-rust.sh writes from the very libraries the app
 * is built with (crates/seedlock-tools): paragraphs separated by a blank line, and headings underlined
 * with "=".
 */
class AboutScreen(app: MainActivity) : Screen(app) {
    override val title = "About SeedLock"

    override fun build(): View {
        val page = Page(app)
        page.heading("SeedLock ${seedlockVersion()}")
        page.text(
            "An offline tool for Bitcoin seeds: encrypted backups, the wallet for Sparrow, signing, multisig. " +
                "Free software under the MIT license. It is not independently audited."
        )
        page.small("Its source code:")
        page.code(SOURCE_CODE)

        val notices = try {
            app.assets.open("notices.txt").use { String(it.readBytes(), Charsets.UTF_8) }
        } catch (e: IOException) {
            null
        }
        if (notices == null) {
            page.warning(
                "The licenses of the libraries SeedLock is built from are missing from this build of the app " +
                    "(android/build-rust.sh writes them before the app is built)."
            )
            return page.view
        }
        // the first paragraph is the notices' title: this page has its own
        val blocks = notices.trim().split("\n\n").drop(1).map { it.trim('\n') }.filter { it.isNotBlank() }
        val section = StringBuilder()
        fun flush() {
            if (section.isNotEmpty()) {
                page.add(TextView(app).apply {
                    text = section.toString()
                    textSize = 13f
                }, 8)
                section.setLength(0)
            }
        }
        for (block in blocks) {
            val lines = block.split("\n")
            if (lines.size == 2 && lines[1].isNotEmpty() && lines[1].all { it == '=' }) {
                flush()
                page.heading(lines[0])
            } else {
                if (section.isNotEmpty()) section.append("\n\n")
                section.append(block)
            }
        }
        flush()
        return page.view
    }
}
