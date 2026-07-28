{
  description = "Dioxus development environment";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs";

  outputs = {
    self,
    nixpkgs,
    ...
  }: let
    system = "x86_64-linux";
    pkgs = import nixpkgs {
      inherit system;
      config = {
        allowUnfree = true;
        android_sdk.accept_license = true;
      };
    };

    sdk =
      (pkgs.androidenv.composeAndroidPackages {
        platformVersions = ["33" "35"];
        buildToolsVersions = ["34.0.0"];
        # includeEmulator = true;
        includeSystemImages = true;
        systemImageTypes = ["google_apis" "google_apis_playstore"];
        abiVersions = ["arm64-v8a"];
        includeNDK = true;
        ndkVersions = ["27.0.12077973"];
      }).androidsdk;
  in {
    devShells.${system}.default = pkgs.mkShell {
      buildInputs = with pkgs; [
        pkg-config
        atkmm
        dioxus-cli
        cairo
        fontconfig
        fribidi
        gdk-pixbuf
        glib
        glib-networking
        gtk3
        gsettings-desktop-schemas
        harfbuzz
        freetype
        libdrm
        libGL
        libgpg-error
        libsoup_3
        mesa
        openssl
        wrapGAppsHook3
        webkitgtk_4_1
        xdotool
        xorg.libX11
        xorg.libxcb
        zlib
        sqlite
        wasm-bindgen-cli
        binaryen
        zenity
        libpng
        libjpeg
        dconf
        shared-mime-info
        librsvg
        gst_all_1.gstreamer
        gst_all_1.gst-plugins-base
        gst_all_1.gst-plugins-good
        gst_all_1.gst-plugins-bad
        gst_all_1.gst-plugins-ugly
        gst_all_1.gst-vaapi
        gst_all_1.gst-libav

        sdk
        openjdk
      ];
      shellHook = ''
        export PKG_CONFIG_PATH=${
          builtins.concatStringsSep ":"
          (map (
              p: "${p.dev or ""}/lib/pkgconfig"
            )
            self.devShells.${system}.default.buildInputs)
        }

        # NixOS GTK needs XDG_DATA_DIRS for schemas/icons
        export XDG_DATA_DIRS="${pkgs.gsettings-desktop-schemas}/share/gsettings-schemas/${pkgs.gsettings-desktop-schemas.name}:${pkgs.gtk3}/share/gsettings-schemas/${pkgs.gtk3.name}:$XDG_DATA_DIRS"

        export ANDROID_HOME="${sdk}/libexec/android-sdk"
        export ANDROID_SDK_ROOT="${sdk}/libexec/android-sdk/"
        export ANDROID_NDK_HOME="${sdk}/libexec/android-sdk/ndk/27.0.12077973"
        export JAVA_HOME="${pkgs.openjdk}"
        export PATH="$PATH:$ANDROID_HOME/emulator:$ANDROID_HOME/platform-tools"
      '';
    };
  };
}
