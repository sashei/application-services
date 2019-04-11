# coding: utf8

# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at http://mozilla.org/MPL/2.0/.

import os.path
from build_config import module_definitions, appservices_version
from decisionlib import *

def main(task_for):
    if task_for == "github-pull-request":
        android_linux_x86_64()
    elif task_for == "github-push":
        android_multiarch()
    elif task_for == "github-release":
        android_multiarch_release()
    else:
        raise ValueError("Unrecognized $TASK_FOR value: %r", task_for)

    full_task_graph = build_full_task_graph()
    populate_chain_of_trust_task_graph(full_task_graph)
    populate_chain_of_trust_required_but_unused_files()

build_artifacts_expire_in = "1 month"
build_dependencies_artifacts_expire_in = "3 month"
log_artifacts_expire_in = "1 year"

build_env = {
    "RUST_BACKTRACE": "1",
    "RUSTFLAGS": "-Dwarnings",
    "CARGO_INCREMENTAL": "0",
    "CI": "1",
}
linux_build_env = {
    "TERM": "dumb",  # Keep Gradle output sensible.
    "CCACHE": "sccache",
    "RUSTC_WRAPPER": "sccache",
    "SCCACHE_IDLE_TIMEOUT": "1200",
    "SCCACHE_CACHE_SIZE": "40G",
    "SCCACHE_ERROR_LOG": "/build/sccache.log",
    "RUST_LOG": "sccache=info",
}

# Calls "$PLATFORM_libs" functions and returns
# their tasks IDs.
def libs_for(*platforms):
    return list(map(lambda p: globals()[p + "_libs"](), platforms))

def android_libs():
    return (
        linux_build_task("Android libs (all architectures): build")
        .with_script("""
            pushd libs
            ./build-all.sh android
            popd
            tar -czf /build/repo/target.tar.gz libs/android
        """)
        .with_artifacts(
            "/build/repo/target.tar.gz",
        )
        .find_or_create("build.libs.android." + CONFIG.git_sha_for_directory("libs"))
    )

def desktop_linux_libs():
    return (
        linux_build_task("Desktop libs (Linux): build")
        .with_script("""
            pushd libs
            ./build-all.sh desktop
            popd
            tar -czf /build/repo/target.tar.gz libs/desktop
        """)
        .with_artifacts(
            "/build/repo/target.tar.gz",
        )
        .find_or_create("build.libs.desktop.linux." + CONFIG.git_sha_for_directory("libs"))
    )

def desktop_macos_libs():
    return (
        linux_cross_compile_build_task("Desktop libs (macOS): build")
        .with_script("""
            pushd libs
            ./build-all.sh darwin
            popd
            tar -czf /build/repo/target.tar.gz libs/desktop
        """)
        .with_artifacts(
            "/build/repo/target.tar.gz",
        )
        .find_or_create("build.libs.desktop.macos." + CONFIG.git_sha_for_directory("libs"))
    )

def desktop_win32_x86_64_libs():
    return (
        linux_build_task("Desktop libs (win32-x86-64): build")
        .with_script("""
            apt-get install --quiet --yes --no-install-recommends mingw-w64
            pushd libs
            ./build-all.sh win32-x86-64
            popd
            tar -czf /build/repo/target.tar.gz libs/desktop
        """)
        .with_artifacts(
            "/build/repo/target.tar.gz",
        )
        .find_or_create("build.libs.desktop.win32-x86-64." + CONFIG.git_sha_for_directory("libs"))
    )

def android_task(task_name, libs_tasks):
    task = linux_cross_compile_build_task(task_name)
    for libs_task in libs_tasks:
        task.with_curl_artifact_script(libs_task, "target.tar.gz")
        task.with_script("tar -xzf target.tar.gz")
    return task

def ktlint_detekt():
    linux_build_task("detekt").with_script("./gradlew --no-daemon clean detekt").create()
    linux_build_task("ktlint").with_script("./gradlew --no-daemon clean ktlint").create()

def android_linux_x86_64():
    ktlint_detekt()
    libs_tasks = libs_for("android", "desktop_linux", "desktop_macos", "desktop_win32_x86_64")
    task = (
        android_task("Build and test (Android - linux-x86-64)", libs_tasks)
        .with_script("""
            echo "rust.targets=linux-x86-64" > local.properties
        """)
        .with_script("""
            yes | sdkmanager --update
            yes | sdkmanager --licenses
            ./gradlew --no-daemon clean
            ./gradlew --no-daemon testDebug
        """)
    )
    for module_info in module_definitions():
        module = module_info['name']
        if module.endswith("-megazord"):
            task.with_script("./automation/check_megazord.sh {}".format(module[0:-9]))
    return task.create()

def gradle_module_task_name(module, gradle_task_name):
    return ":%s:%s" % (module, gradle_task_name)

def gradle_module_task(libs_tasks, module_info, is_release):
    module = module_info['name']
    if is_release:
        task_title = "{} - Build, test and upload to bintray".format(module)
    else:
        task_title = "{} - Build and test".format(module)
    task = (
        android_task(task_title, libs_tasks)
        .with_script("""
            yes | sdkmanager --update
            yes | sdkmanager --licenses
            ./gradlew --no-daemon clean
        """)
        .with_script("./gradlew --no-daemon {}".format(gradle_module_task_name(module, "testDebug")))
        .with_script("./gradlew --no-daemon {}".format(gradle_module_task_name(module, "assembleRelease")))
        .with_script("./gradlew --no-daemon {}".format(gradle_module_task_name(module, "publish")))
        .with_script("./gradlew --no-daemon {}".format(gradle_module_task_name(module, "zipMavenArtifacts")))
    )
    for artifact_info in module_info['artifacts']:
        task.with_artifacts(artifact_info['artifact'])
    if is_release:
        if module_info['uploadSymbols']:
            task.with_scopes("secrets:get:project/application-services/symbols-token")
            task.with_script("./automation/upload_android_symbols.sh {}".format(module_info['path']))
        task.with_scopes("secrets:get:project/application-services/publish")
        task.with_script("python automation/taskcluster/release/fetch-bintray-api-key.py")
        task.with_script('./gradlew --no-daemon {} --debug -PvcsTag="$GIT_SHA"'.format(gradle_module_task_name(module, "bintrayUpload")))
    return task.create()

def build_gradle_modules_tasks(is_release):
    libs_tasks = libs_for("android", "desktop_linux", "desktop_macos", "desktop_win32_x86_64")
    module_build_tasks = {}
    for module_info in module_definitions():
        module_build_tasks[module_info['name']] = gradle_module_task(libs_tasks, module_info, is_release)
    return module_build_tasks

def android_multiarch():
    ktlint_detekt()
    build_gradle_modules_tasks(False)

def android_multiarch_release():
    module_build_tasks = build_gradle_modules_tasks(True)
    return (
        linux_build_task("All modules - Publish via bintray")
        .with_dependencies(*module_build_tasks.values())
        # Our -unpublished- artifacts were uploaded in build_gradle_modules_tasks(),
        # however there is not way to just trigger a bintray publish from gradle without
        # uploading anything, so we do it manually using curl :(
        # We COULD publish each artifact individually, however that would mean if
        # a build task fails we end up with a partial release.
        # Since we manipulate secrets, we also disable bash debug mode.
        .with_script("""
            python automation/taskcluster/release/fetch-bintray-api-key.py
            set +x
            BINTRAY_USER=$(grep 'bintray.user=' local.properties | cut -d'=' -f2)
            BINTRAY_APIKEY=$(grep 'bintray.apikey=' local.properties | cut -d'=' -f2)
            PUBLISH_URL=https://api.bintray.com/content/mozilla-appservices/application-services/org.mozilla.appservices/{}/publish
            echo "Publishing on $PUBLISH_URL"
            curl -X POST -u $BINTRAY_USER:$BINTRAY_APIKEY $PUBLISH_URL
            echo "Success!"
            set -x
        """.format(appservices_version()))
        .with_scopes("secrets:get:project/application-services/publish")
        .with_features('taskclusterProxy') # So we can fetch the bintray secret.
        .create()
    )

def dockerfile_path(name):
    return os.path.join(os.path.dirname(__file__), "docker", name + ".dockerfile")


def linux_task(name):
    return DockerWorkerTask(name).with_worker_type("application-services-r")


def linux_build_task(name):
    return (
        linux_task(name)
        # https://docs.taskcluster.net/docs/reference/workers/docker-worker/docs/caches
        .with_scopes("docker-worker:cache:application-services-*")
        .with_caches(**{
            "application-services-cargo-registry": "/root/.cargo/registry",
            "application-services-cargo-git": "/root/.cargo/git",
            "application-services-sccache": "/root/.cache/sccache",
            "application-services-gradle": "/root/.gradle",
            "application-services-rustup": "/root/.rustup",
            "application-services-android-ndk-toolchain": "/root/.android-ndk-r15c-toolchain",
            "application-services-rust-target": "/build/repo/target",
        })
        .with_index_and_artifacts_expire_in(build_artifacts_expire_in)
        .with_artifacts("/build/sccache.log")
        .with_max_run_time_minutes(120)
        .with_dockerfile(dockerfile_path("build"))
        .with_env(**build_env, **linux_build_env)
        .with_script("""
            rustup toolchain install stable
            rustup default stable
            # rustup target add x86_64-unknown-linux-gnu # See https://github.com/rust-lang-nursery/rustup.rs/issues/1533.

            rustup target add x86_64-linux-android
            rustup target add i686-linux-android
            rustup target add armv7-linux-androideabi
            rustup target add aarch64-linux-android
        """)
        # We run the following script so the cached target/ folder doesn't grow too big.
        # See https://github.com/rust-lang/cargo/issues/5026
        .with_script("""
            RUST_VERSION=$(rustc --version)
            if [ ! -f ./target/.rust-version ] || [ "$RUST_VERSION" != "$(cat ./target/.rust-version)" ]; then
                echo "target/ contains artifacts that were generated by a different Rust version than the current one, purging directory."
                rm -rf ./target/*
                echo "$RUST_VERSION" > ./target/.rust-version
            fi
        """)
        .with_script("""
            test -d $ANDROID_NDK_TOOLCHAIN_DIR/arm-$ANDROID_NDK_API_VERSION    || $ANDROID_NDK_ROOT/build/tools/make_standalone_toolchain.py --arch="arm"   --api="$ANDROID_NDK_API_VERSION" --install-dir="$ANDROID_NDK_TOOLCHAIN_DIR/arm-$ANDROID_NDK_API_VERSION" --deprecated-headers --force
            test -d $ANDROID_NDK_TOOLCHAIN_DIR/arm64-$ANDROID_NDK_API_VERSION  || $ANDROID_NDK_ROOT/build/tools/make_standalone_toolchain.py --arch="arm64" --api="$ANDROID_NDK_API_VERSION" --install-dir="$ANDROID_NDK_TOOLCHAIN_DIR/arm64-$ANDROID_NDK_API_VERSION" --deprecated-headers --force
            test -d $ANDROID_NDK_TOOLCHAIN_DIR/x86-$ANDROID_NDK_API_VERSION    || $ANDROID_NDK_ROOT/build/tools/make_standalone_toolchain.py --arch="x86"   --api="$ANDROID_NDK_API_VERSION" --install-dir="$ANDROID_NDK_TOOLCHAIN_DIR/x86-$ANDROID_NDK_API_VERSION" --deprecated-headers --force
            test -d $ANDROID_NDK_TOOLCHAIN_DIR/x86_64-$ANDROID_NDK_API_VERSION || $ANDROID_NDK_ROOT/build/tools/make_standalone_toolchain.py --arch="x86_64"   --api="$ANDROID_NDK_API_VERSION" --install-dir="$ANDROID_NDK_TOOLCHAIN_DIR/x86_64-$ANDROID_NDK_API_VERSION" --deprecated-headers --force
        """)
        .with_repo()
        .with_script("""
            ./libs/verify-android-environment.sh
        """)
    )

def linux_cross_compile_build_task(name):
    return (
        linux_build_task(name)
        .with_scopes('project:releng:services/tooltool/api/download/internal')
        .with_features('taskclusterProxy') # So we can fetch from tooltool.
        .with_script("""
            rustup target add x86_64-apple-darwin

            pushd libs
            ./cross-compile-macos-on-linux-desktop-libs.sh
            popd

            # Rust requires dsymutil on the PATH: https://github.com/rust-lang/rust/issues/52728.
            export PATH=$PATH:/tmp/clang/bin

            export ORG_GRADLE_PROJECT_RUST_ANDROID_GRADLE_TARGET_X86_64_APPLE_DARWIN_SQLCIPHER_LIB_DIR=/build/repo/libs/desktop/darwin/sqlcipher/lib
            export ORG_GRADLE_PROJECT_RUST_ANDROID_GRADLE_TARGET_X86_64_APPLE_DARWIN_OPENSSL_DIR=/build/repo/libs/desktop/darwin/openssl
            export ORG_GRADLE_PROJECT_RUST_ANDROID_GRADLE_TARGET_X86_64_APPLE_DARWIN_CC=/tmp/clang/bin/clang
            export ORG_GRADLE_PROJECT_RUST_ANDROID_GRADLE_TARGET_X86_64_APPLE_DARWIN_TOOLCHAIN_PREFIX=/tmp/cctools/bin
            export ORG_GRADLE_PROJECT_RUST_ANDROID_GRADLE_TARGET_X86_64_APPLE_DARWIN_AR=/tmp/cctools/bin/x86_64-apple-darwin11-ar
            export ORG_GRADLE_PROJECT_RUST_ANDROID_GRADLE_TARGET_X86_64_APPLE_DARWIN_RANLIB=/tmp/cctools/bin/x86_64-apple-darwin11-ranlib
            export ORG_GRADLE_PROJECT_RUST_ANDROID_GRADLE_TARGET_X86_64_APPLE_DARWIN_LD_LIBRARY_PATH=/tmp/clang/lib
            export ORG_GRADLE_PROJECT_RUST_ANDROID_GRADLE_TARGET_X86_64_APPLE_DARWIN_RUSTFLAGS="-C linker=/tmp/clang/bin/clang -C link-arg=-B -C link-arg=/tmp/cctools/bin -C link-arg=-target -C link-arg=x86_64-apple-darwin11 -C link-arg=-isysroot -C link-arg=/tmp/MacOSX10.11.sdk -C link-arg=-Wl,-syslibroot,/tmp/MacOSX10.11.sdk -C link-arg=-Wl,-dead_strip"
            # For ring's use of `cc`.
            export ORG_GRADLE_PROJECT_RUST_ANDROID_GRADLE_TARGET_X86_64_APPLE_DARWIN_CFLAGS_x86_64_apple_darwin="-B /tmp/cctools/bin -target x86_64-apple-darwin11 -isysroot /tmp/MacOSX10.11.sdk -Wl,-syslibroot,/tmp/MacOSX10.11.sdk -Wl,-dead_strip"

            apt-get install --quiet --yes --no-install-recommends mingw-w64
            rustup target add x86_64-pc-windows-gnu
            export ORG_GRADLE_PROJECT_RUST_ANDROID_GRADLE_TARGET_X86_64_PC_WINDOWS_GNU_RUSTFLAGS="-C linker=x86_64-w64-mingw32-gcc"
            export ORG_GRADLE_PROJECT_RUST_ANDROID_GRADLE_TARGET_X86_64_PC_WINDOWS_GNU_AR=x86_64-w64-mingw32-ar
            export ORG_GRADLE_PROJECT_RUST_ANDROID_GRADLE_TARGET_X86_64_PC_WINDOWS_GNU_CC=x86_64-w64-mingw32-gcc
        """)
    )

CONFIG.task_name_template = "Application Services - %s"
CONFIG.index_prefix = "project.application-services.application-services"
CONFIG.docker_image_build_worker_type = "application-services-r"
CONFIG.docker_images_expire_in = build_dependencies_artifacts_expire_in
CONFIG.repacked_msi_files_expire_in = build_dependencies_artifacts_expire_in


if __name__ == "__main__":  # pragma: no cover
    main(task_for=os.environ["TASK_FOR"])
