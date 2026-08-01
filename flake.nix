{
  description = "WACV - WhatsApp Chat Viewer (Dioxus)";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = {
    nixpkgs,
    rust-overlay,
    ...
  }: let
    system = "x86_64-linux";
    pkgs = import nixpkgs {
      inherit system;
      overlays = [rust-overlay.overlays.default];
      config = {
        allowUnfree = true;
        android_sdk.accept_license = true;
      };
    };
    inherit (pkgs) lib;

    sdk =
      (pkgs.androidenv.composeAndroidPackages {
        platformVersions = ["33" "35"];
        buildToolsVersions = ["34.0.0"];
        includeSystemImages = true;
        systemImageTypes = ["google_apis" "google_apis_playstore"];
        abiVersions = ["arm64-v8a"];
        includeNDK = true;
        ndkVersions = ["27.0.12077973"];
      }).androidsdk;

    nativeBuildInputs = {
      linux = with pkgs; [
        pkg-config
        wrapGAppsHook3
        makeWrapper
      ];
      android = with pkgs; [
        RUSTC
        cargo-ndk
        pkg-config
        openjdk
        sdk
      ];
    };

    buildInputs = with pkgs; [
      atk
      atkmm
      cairo
      fontconfig
      fribidi
      gdk-pixbuf
      glib
      glib-networking
      gtk3
      pango
      gsettings-desktop-schemas
      harfbuzz
      freetype
      libdrm
      libGL
      libgpg-error
      libsoup_3
      mesa
      openssl
      webkitgtk_4_1
      wayland
      xdotool
    ] ++ gstPlugins;

    gstPlugins = with pkgs.gst_all_1; [
      gst-plugins-base
      gst-plugins-good
      gst-plugins-bad
      gst-plugins-ugly
      gst-vaapi
      gst-libav
    ];

    gstPluginPath = lib.makeSearchPath "lib/gstreamer-1.0" gstPlugins;

    RUSTC = pkgs.rust-bin.stable.latest.default.override {
      targets = ["aarch64-linux-android"];
    };

    PKG_CONFIG_PATH =
      lib.concatStringsSep ":"
      (map (p: "${p.dev}/lib/pkgconfig")
        (lib.filter (p: p ? dev) buildInputs))
      + ":${pkgs.zlib.dev}/share/pkgconfig:${pkgs.xdotool}/lib/pkgconfig";
    XDG_DATA_DIRS = "${pkgs.gsettings-desktop-schemas}/share/gsettings-schemas/${pkgs.gsettings-desktop-schemas.name}:${pkgs.gtk3}/share/gsettings-schemas/${pkgs.gtk3.name}:$XDG_DATA_DIRS";

    ANDROID_HOME = "${sdk}/libexec/android-sdk";
    ANDROID_SDK_ROOT = "${sdk}/libexec/android-sdk/";
    ANDROID_NDK_HOME = "${sdk}/libexec/android-sdk/ndk/27.0.12077973";
    JAVA_HOME = "${pkgs.openjdk}";

    GRADLE_OPTS = "-Djdk.map.althashing.threshold=-1 -Dorg.gradle.project.android.aapt2FromMavenOverride=${sdk}/libexec/android-sdk/build-tools/34.0.0/aapt2";
  in {
    devShells.${system}.default = pkgs.mkShell {
      nativeBuildInputs = nativeBuildInputs.linux ++ nativeBuildInputs.android ++ [pkgs.dioxus-cli];
      inherit
        buildInputs
        XDG_DATA_DIRS
        ANDROID_HOME
        ANDROID_NDK_HOME
        ANDROID_SDK_ROOT
        JAVA_HOME
        PKG_CONFIG_PATH
        GRADLE_OPTS
        ;
    };

    packages.${system} = let
      pname = "wacv";
      version = "0.1.0";
      src = ./.;

      cargoLock.lockFile = ./Cargo.lock;
    in {
      desktop = pkgs.rustPlatform.buildRustPackage {
        inherit
          pname
          version
          src
          buildInputs
          PKG_CONFIG_PATH
          cargoLock
          ;

        nativeBuildInputs = nativeBuildInputs.linux;
        makeWrapperArgs = [
          "--prefix GST_PLUGIN_SYSTEM_PATH_1_0 : ${gstPluginPath}"
        ];
      };

      android = let
        gradleDeps = pkgs.stdenv.mkDerivation {
          dontFixup = true;
          pname = "wacv-gradle-deps";
          inherit version src;
          nativeBuildInputs = [pkgs.openjdk sdk];
          inherit
            ANDROID_HOME
            ANDROID_SDK_ROOT
            ANDROID_NDK_HOME
            JAVA_HOME
            GRADLE_OPTS
            ;

          outputHashMode = "recursive";
          outputHash = "sha256-+q2hw5QzcWKwuOBA2clR6g3MP21UjkLLRPuXV8tWTug=";

          buildPhase = ''
            export HOME="$TMPDIR"
            export GRADLE_USER_HOME="$TMPDIR/.gradle"
            export ANDROID_USER_HOME="$TMPDIR/.android"
            (cd android && ./gradlew --no-daemon \
              -Pkotlin.compiler.execution.strategy=in-process assembleDebug)
          '';

          installPhase = ''
            mkdir -p $out
            cp -a "$GRADLE_USER_HOME"/. $out/

            find $out -name '*.lock' -delete
            find $out -name '*.lck' -delete
            rm -rf $out/daemon $out/.tmp $out/kotlin-profile $out/notifications \
                   $out/caches/journal-1 $out/caches/build-cache-1

            rm -rf $out/caches/9.1.0 $out/caches/jars-9
            rm -f $out/caches/modules-2/gc.properties \
                  $out/caches/gc.properties
          '';
        };
      in
        pkgs.rustPlatform.buildRustPackage {
          inherit
            pname
            version
            src
            ANDROID_HOME
            ANDROID_SDK_ROOT
            ANDROID_NDK_HOME
            JAVA_HOME
            PKG_CONFIG_PATH
            cargoLock
            buildInputs
            GRADLE_OPTS
            gradleDeps
            ;

          dontCargoCheck = true;

          nativeBuildInputs = nativeBuildInputs.android;

          buildPhase = ''
            runHook preBuild
            ${RUSTC}/bin/cargo ndk build --target aarch64-linux-android --release --lib
            runHook postBuild
          '';

          installPhase = ''
            runHook preInstall

            mkdir -p android/app/src/main/jniLibs/arm64-v8a
            cp target/aarch64-linux-android/release/libdioxusmain.so android/app/src/main/jniLibs/arm64-v8a/libdioxusmain.so

            export HOME="$TMPDIR"
            export GRADLE_USER_HOME="$TMPDIR/.gradle"
            export ANDROID_USER_HOME="$TMPDIR/.android"

            cp -a "$gradleDeps"/. "$GRADLE_USER_HOME"/
            chmod -R u+w "$GRADLE_USER_HOME"

            (cd android && ./gradlew --offline --no-daemon \
            -Pkotlin.compiler.execution.strategy=in-process assembleDebug)

            mkdir -p $out/apk
            cp android/app/build/outputs/apk/debug/app-debug.apk $out/apk/wacv.apk

            runHook postInstall
          '';
        };
    };
  };
}
