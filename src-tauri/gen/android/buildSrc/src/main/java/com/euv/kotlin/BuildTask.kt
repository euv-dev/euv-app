import java.io.File
import org.apache.tools.ant.taskdefs.condition.Os
import org.gradle.api.DefaultTask
import org.gradle.api.GradleException
import org.gradle.api.logging.LogLevel
import org.gradle.api.tasks.Input
import org.gradle.api.tasks.TaskAction

open class BuildTask : DefaultTask() {
    @Input
    var rootDirRel: String? = null
    @Input
    var target: String? = null
    @Input
    var release: Boolean? = null

    @TaskAction
    fun assemble() {
        // Rust libs are already compiled by `tauri android build --ci`.
        // Skip recompilation to avoid WebSocket connection issues when running Gradle directly.
        project.logger.info("Skipping Rust build (already compiled): target=$target release=$release")
    }
}
